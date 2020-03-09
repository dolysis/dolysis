use {
    super::*,
    serde_yaml::from_reader as read_yaml,
    std::{fmt, io},
};

#[derive(Debug)]
pub struct JoinSet {
    store: Arena<Node<FilterData>>,
    set: JoinInner,
}

impl JoinSet {
    const VALID_INPUT_KINDS: &'static [(bool, bool, bool)] =
        &[StartEnd::TARGET, StartWhile::TARGET, While::TARGET];

    pub fn new_filter<R>(data: R) -> Result<Self, LoadError>
    where
        R: io::Read,
    {
        let wrap: JoinWrap = read_yaml(data)?;
        trace!("Yaml syntax valid");

        let mut store = Arena::new();
        let JoinIntermediate { start, cont, end } = wrap.join;

        let set = Some((start, cont, end))
            .map(|(s, c, e)| {
                enter!(span, always_span!("init.join", name = field::Empty));
                (
                    s.map(|seeds| {
                        span.record("name", &"Start");
                        init_tree(&mut store, seeds)
                    }),
                    c.map(|seeds| {
                        span.record("name", &"While");
                        init_tree(&mut store, seeds)
                    }),
                    e.map(|seeds| {
                        span.record("name", &"End");
                        init_tree(&mut store, seeds)
                    }),
                )
            })
            .map(|input| JoinInner::new(Self::VALID_INPUT_KINDS, input))
            // Note the unwrap removes the Some added above, and is thus always safe
            .unwrap()?;

        Ok(Self { store, set })
    }

    pub fn new_handle(&self) -> JoinSetHandle {
        JoinSetHandle::new(self)
    }

    pub(in super::super) fn print_valid_input(f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut valid = Self::VALID_INPUT_KINDS.iter().peekable();

        write!(f, "[")?;
        loop {
            let item = valid.next();
            let last = valid.peek().is_none();

            match (item, last) {
                (Some(input), false) => write!(
                    f,
                    "({}, {}, {}), ",
                    input.0 as u8, input.1 as u8, input.2 as u8
                )?,
                (Some(input), true) => write!(
                    f,
                    "({}, {}, {})",
                    input.0 as u8, input.1 as u8, input.2 as u8
                )?,
                (None, _) => break,
            }
        }

        write!(f, "]")
    }
}

#[derive(Debug)]
pub struct JoinSetHandle<'j> {
    store: &'j Arena<Node<FilterData>>,
    handle: JoinInner,
    state: Option<()>,
}

impl<'j> JoinSetHandle<'j> {
    pub fn should_join<T>(&mut self, on: T) -> bool
    where
        T: AsRef<str>,
    {
        let store = self.store;
        Self::internal_join(&mut self.handle, &mut self.state, store, on)
    }

    fn new(parent: &'j JoinSet) -> Self {
        JoinSetHandle {
            store: &parent.store,
            handle: parent.set,
            state: Some(()),
        }
    }

    // Required to help brwck isolate references
    fn internal_join<T>(
        handle: &mut JoinInner,
        state: &mut Option<()>,
        store: &Arena<Node<FilterData>>,
        on: T,
    ) -> bool
    where
        T: AsRef<str>,
    {
        let item = on.as_ref();

        handle.join(state, |idx| {
            store
                .get(idx)
                .unwrap()
                .traverse_with(&|a, d, e| recursive_match(a, d, e, item), store)
        })
    }
}

trait Join {
    type State;

    fn join<F>(&mut self, cxt: &mut Self::State, f: F) -> bool
    where
        F: Fn(Index) -> bool;
}

#[derive(Debug, Clone, Copy)]
enum JoinInner {
    StartEnd(StartEnd),
    StartWhile(StartWhile),
    While(While),
}

impl JoinInner {
    fn new(
        whitelist: &[(bool, bool, bool)],
        input: (Option<Index>, Option<Index>, Option<Index>),
    ) -> Result<Self, LoadError> {
        let (start, cont, end) = input;
        let target = (start.is_some(), cont.is_some(), end.is_some());

        whitelist
            .iter()
            .fold(None, |mut out, &valid| {
                if out.is_none() && valid == target {
                    out = Some(Self::instantiate_unchecked(input));
                    out
                } else {
                    out
                }
            })
            .ok_or_else(|| target.into())
    }

    fn instantiate_unchecked(input: (Option<Index>, Option<Index>, Option<Index>)) -> Self {
        let (start, cont, end) = input;
        let target = (start.is_some(), cont.is_some(), end.is_some());

        match input {
            (start, _, end) if target == StartEnd::TARGET => {
                Self::StartEnd(StartEnd(start.unwrap(), end.unwrap()))
            }
            (start, cont, _) if target == StartWhile::TARGET => {
                Self::StartWhile(StartWhile(start.unwrap(), cont.unwrap()))
            }
            (_, cont, _) if target == While::TARGET => Self::While(While(cont.unwrap())),
            _ => unreachable!(
                "Bad Join data, the caller should guarantee this branch is unreachable"
            ),
        }
    }
}

impl Join for JoinInner {
    type State = Option<()>;

    fn join<F>(&mut self, cxt: &mut Self::State, f: F) -> bool
    where
        F: Fn(Index) -> bool,
    {
        match self {
            Self::StartEnd(inner) => inner.join(cxt, f),
            Self::StartWhile(inner) => inner.join(cxt, f),
            Self::While(inner) => inner.join(cxt, f),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct StartEnd(Index, Index);

impl StartEnd {
    const TARGET: (bool, bool, bool) = (true, false, true);
}

impl Join for StartEnd {
    // Some = Start, None = End
    type State = Option<()>;

    fn join<F>(&mut self, cxt: &mut Self::State, f: F) -> bool
    where
        F: Fn(Index) -> bool,
    {
        let current = cxt.as_mut().map(|_| self.0).unwrap_or(self.1);

        let outcome = f(current);

        match (outcome, *cxt) {
            // If Start matches we now match on End until it returns true
            (true, Some(_)) => *cxt = None,
            // If End matches we're done with this join, reset state to Start
            (true, None) => *cxt = Some(()),
            // All other outcomes do not require a state change
            _ => (),
        };

        outcome
    }
}

#[derive(Debug, Clone, Copy)]
struct StartWhile(Index, Index);

impl StartWhile {
    const TARGET: (bool, bool, bool) = (true, true, false);
}

impl Join for StartWhile {
    // Some = Start, None = While
    type State = Option<()>;

    fn join<F>(&mut self, cxt: &mut Self::State, f: F) -> bool
    where
        F: Fn(Index) -> bool,
    {
        let current = cxt.as_mut().map(|_| self.0).unwrap_or(self.1);

        let outcome = f(current);

        match (outcome, *cxt) {
            // If Start matches we now match on While until it returns false
            (true, Some(_)) => *cxt = None,
            // If While returns false, we're done with this join, reset state to Start
            (false, None) => *cxt = Some(()),
            // All other outcomes do not require a state change
            _ => (),
        };

        outcome
    }
}
#[derive(Debug, Clone, Copy)]
struct While(Index);

impl While {
    const TARGET: (bool, bool, bool) = (false, true, false);
}

impl Join for While {
    type State = Option<()>;

    fn join<F>(&mut self, _: &mut Self::State, f: F) -> bool
    where
        F: Fn(Index) -> bool,
    {
        // No state required for While only
        f(self.0)
    }
}

#[derive(Deserialize, Debug)]
struct JoinWrap {
    join: JoinIntermediate,
}

#[derive(Deserialize, Debug)]
struct JoinIntermediate {
    start: Option<Vec<FilterSeed>>,
    #[serde(rename = "while")]
    cont: Option<Vec<FilterSeed>>,
    end: Option<Vec<FilterSeed>>,
}
