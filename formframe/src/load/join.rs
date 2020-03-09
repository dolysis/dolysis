use {
    super::{
        error::{Err, LoadError},
        filter::{serial_traverse, FilterData, FilterSeed},
    },
    crate::{graph::Node, prelude::*},
    generational_arena::{Arena, Index},
    regex::Regex,
    serde::{de, Deserialize, Deserializer},
    serde_yaml::from_reader as read_yaml,
    std::{collections::HashMap, fmt, io},
};

pub struct JoinSet {
    store: Arena<Node<FilterData>>,
    first: Option<Index>,
    while_: Option<Index>,
    last: Option<Index>,
}

impl JoinSet {
    const VALID_INPUT_KINDS: &'static [(bool, bool, bool)] = &[
        (true, true, true),
        (true, false, true),
        (true, true, false),
        (false, true, false),
    ];

    // pub fn new_filter<R>(data: R) -> Result<Self, LoadError>
    // where
    //     R: io::Read,
    // {
    //     let wrap: JoinWrap = read_yaml(data)?;
    //     let JoinIntermediate { start, cont, end } = wrap.join;

    //     trace!("Yaml syntax valid");

    //     let mut store = Arena::new();
    //     let mut set = HashMap::new();

    //     match (start, cont, end) {
    //         (Some(start), Some(cont), Some(end)) => (),
    //         (Some(start), None, Some(end)) => (),
    //         (Some(start), Some(cont), None) => (),
    //         (None, Some(cont), None) => (),
    //         invalid => {
    //             return Err((
    //                 invalid.0.is_some(),
    //                 invalid.1.is_some(),
    //                 invalid.2.is_some(),
    //             )
    //                 .into())
    //         }
    //     }

    //     Ok(Self {
    //         named_set: set,
    //         store,
    //     })
    // }

    pub(super) fn print_valid_input(f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

trait StatefulJoin {
    type State;

    fn join<F>(&mut self, cxt: &mut Self::State, f: F) -> bool
    where
        F: Fn(Index) -> bool;
}

#[derive(Debug, Clone, Copy)]
enum JoinInner {
    StartWhileEnd(StartWhileEnd),
    StartEnd(StartEnd),
    StartWhile(StartWhile),
    While(While),
}

#[derive(Debug, Clone, PartialEq)]
enum Context {
    Start,
    While,
    End,
}

#[derive(Debug, Clone, Copy)]
struct StartWhileEnd(Index, Index, Index);

#[derive(Debug, Clone, Copy)]
struct StartEnd(Index, Index);
#[derive(Debug, Clone, Copy)]
struct StartWhile(Index, Index);
#[derive(Debug, Clone, Copy)]
struct While(Index);

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
