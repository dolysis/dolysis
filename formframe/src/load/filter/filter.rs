use {
    super::*,
    serde_yaml::from_reader as read_yaml,
    std::{collections::HashMap, convert::TryFrom, io},
};

#[derive(Debug, Deserialize)]
#[serde(try_from = "FilterWrap")]
pub struct FilterSet {
    named_set: HashMap<String, Index>,
    store: Arena<Node<FilterData>>,
}

impl FilterSet {
    pub fn new_filter<R>(data: R) -> Result<Self, LoadError>
    where
        R: io::Read,
    {
        read_yaml(data).map_err(|e| e.into())
    }

    pub fn access_set<F, T>(&self, f: F) -> T
    where
        F: Fn(&Arena<Node<FilterData>>, &HashMap<String, Index>) -> T,
        T: Sized + Send + Sync,
    {
        f(&self.store, &self.named_set)
    }

    pub fn is_match_all<T>(&self, on: T) -> bool
    where
        T: AsRef<str>,
    {
        let on = on.as_ref();
        self.access_set(|store, m| {
            m.values().fold(true, |state, root| {
                if state == false {
                    state
                } else {
                    store
                        .get(*root)
                        .unwrap()
                        .traverse_with(&|s, f, e| recursive_match(s, f, e, on), store)
                }
            })
        })
    }

    pub fn is_match_with<T>(&self, name: &str, on: T) -> bool
    where
        T: AsRef<str>,
    {
        let on = on.as_ref();
        self.access_set(|store, m| {
            let root = m.get(name).unwrap();
            store
                .get(*root)
                .unwrap()
                .traverse_with(&|s, f, e| recursive_match(s, f, e, on), store)
        })
    }
}

impl TryFrom<FilterWrap> for FilterSet {
    type Error = LoadError;

    fn try_from(wrap: FilterWrap) -> Result<Self, Self::Error> {
        let mut store = Arena::new();
        let mut set = HashMap::new();

        wrap.filter.into_iter().try_for_each(|(name, seeds)| {
            enter!(always_span!("init.filter", name = name.as_str()));
            set.insert(name.clone(), init_tree(&mut store, seeds))
                .map_or_else(|| Ok(()), |_| Err(Err::DuplicateRootName(name)))
        })?;

        Ok(Self {
            named_set: set,
            store,
        })
    }
}

#[derive(Deserialize, Debug)]
struct FilterWrap {
    filter: FilterIntermediate,
}

type FilterIntermediate = HashMap<String, Vec<FilterSeed>>;
