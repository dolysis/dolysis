use {
    super::*,
    serde_yaml::from_reader as read_yaml,
    std::{collections::HashMap, io},
};

#[derive(Debug)]
pub struct FilterSet {
    named_set: HashMap<String, Index>,
    store: Arena<Node<FilterData>>,
}

impl FilterSet {
    pub fn new_filter<R>(data: R) -> Result<Self, LoadError>
    where
        R: io::Read,
    {
        let wrap: FilterWrap = read_yaml(data)?;

        trace!("Yaml syntax valid");

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
}

#[derive(Deserialize, Debug)]
struct FilterWrap {
    filter: FilterIntermediate,
}

type FilterIntermediate = HashMap<String, Vec<FilterSeed>>;
