use {
    super::error::LoadError,
    crate::graph::Node,
    generational_arena::{Arena, Index},
    regex::Regex,
    serde::{de, Deserialize, Deserializer},
    serde_yaml::from_reader as read_yaml,
    std::{collections::HashMap, error, io},
};

pub use serial_traverse as is_match;

#[derive(Debug)]
pub struct FilterSet {
    named_set: HashMap<String, Index>,
    store: Arena<Node<FilterData>>,
}

impl FilterSet {
    pub fn try_new<R>(data: R) -> Result<Self, LoadError>
    where
        R: io::Read,
    {
        let wrap: FWrap = read_yaml(data)?;
        let mut store = Arena::new();
        let mut set = HashMap::new();

        wrap.filter.into_iter().try_for_each(|(name, seeds)| {
            set.insert(name.clone(), init_tree(&mut store, seeds))
                .map_or_else(|| Ok(()), |_| Err(LoadError::DuplicateRootName(name)))
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
}

pub fn serial_traverse(
    arena: &Arena<Node<FilterData>>,
    data: &FilterData,
    edges: &[Index],
    text: &str,
) -> bool {
    match data.ty {
        // Run regex
        NodeType::Regex(ref rx) => {
            let b = rx.is_match(text);
            println!("RX: {}, = {}", rx, b.negate(data.negate));
            b.negate(data.negate)
        }
        // Wait for all success / return on first error
        NodeType::And => {
            let res: Result<(), ()> = edges
                .into_iter()
                .map(|idx| {
                    arena
                        .get(*idx)
                        .unwrap()
                        .traverse_with(&|a, d, i| serial_traverse(a, d, i, text), arena)
                })
                .map(|b| match b {
                    true => Ok(()),
                    // Note that we halt on the first false value, due to Result's FromIter impl
                    false => Err(()),
                })
                .collect();

            res.is_ok().negate(data.negate)
        }
        // Return first success / wait for all failure
        NodeType::Or => {
            let res: Result<(), ()> = edges
                .into_iter()
                .map(|idx| {
                    arena
                        .get(*idx)
                        .unwrap()
                        .traverse_with(&|a, d, i| serial_traverse(a, d, i, text), arena)
                })
                .map(|b| match b {
                    false => Ok(()),
                    // Note that we halt on the first true value, due to Result's FromIter impl
                    true => Err(()),
                })
                .collect();

            res.is_err().negate(data.negate)
        }
    }
}

fn init_tree(arena: &mut Arena<Node<FilterData>>, seeds: Vec<FilterSeed>) -> Index {
    let mut top_level = init_recursive(arena, false, seeds.into_iter());

    match top_level.len() {
        // If the tree is completely empty, return a 'And' root that always returns true
        // TODO: This seems weird... it might be better to error here and bail out
        0 => Node::new(NodeType::And.into(), arena),
        // If there is only one node in the top level return it as the root node
        1 => top_level.pop().unwrap(),
        // Otherwise instantiate a top level 'And' node
        // TODO: Maybe let the user decide whether the top level is an 'And' or an 'Or'
        _ => {
            let root = Node::new_unallocated(NodeType::And.into());
            root.edges.set(top_level).unwrap();
            arena.insert(root)
        }
    }
}

fn init_recursive<I>(arena: &mut Arena<Node<FilterData>>, negate: bool, iter: I) -> Vec<Index>
where
    I: Iterator<Item = FilterSeed>,
{
    let mut edges = Vec::new();

    for seed in iter {
        match seed {
            // A Regex seed will never have children, it is guaranteed to be a leaf node.
            FilterSeed::Regex(rx) => {
                let node = Node::new(FilterData::new(NodeType::Regex(rx), negate), arena);

                edges.push(node);
            }
            // Note that 'Not' seeds are _not_ themselves nodes, they merely invert nodes below and
            // pass them as children to the node above
            FilterSeed::Not(vec) => {
                // Notice we shadow invert whatever the bool was
                let negate = !negate;
                let e = init_recursive(arena, negate, vec.into_iter());

                edges.extend(e);
            }
            // 'And' and 'Or' seeds _are_ nodes, therefore allocate them in the arena and
            // assign them their children, before pushing them to the calling node's children
            seed @ FilterSeed::And(_) | seed @ FilterSeed::Or(_) => {
                let (nt, vec) = match seed {
                    FilterSeed::And(vec) => (NodeType::And, vec),
                    FilterSeed::Or(vec) => (NodeType::Or, vec),
                    // Outer match guarantees other variants will not hit this branch
                    // TODO: maybe change to unreachable_unchecked!()
                    _ => unreachable!(),
                };

                let node = Node::new_unallocated(FilterData::new(nt, negate));
                node.edges
                    .set(init_recursive(arena, negate, vec.into_iter()))
                    .unwrap();
                let node_idx = arena.insert(node);

                edges.push(node_idx);
            }
        }
    }

    edges
}

#[derive(Debug, Clone)]
pub struct FilterData {
    pub ty: NodeType,
    pub negate: BoolExt,
}

impl FilterData {
    pub fn new<T>(ty: NodeType, negate: T) -> Self
    where
        T: Into<BoolExt>,
    {
        Self {
            ty,
            negate: negate.into(),
        }
    }
}

impl From<NodeType> for FilterData {
    fn from(ty: NodeType) -> Self {
        Self::new(ty, false)
    }
}

#[derive(Debug, Clone)]
pub enum NodeType {
    Regex(Regex),
    And,
    Or,
}

/// Extension type for use with Negate
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum BoolExt {
    True = 1,
    False = 0,
}

impl From<bool> for BoolExt {
    fn from(b: bool) -> Self {
        match b {
            true => Self::True,
            false => Self::False,
        }
    }
}

impl Into<bool> for BoolExt {
    fn into(self) -> bool {
        match self {
            Self::True => true,
            Self::False => false,
        }
    }
}

impl Default for BoolExt {
    fn default() -> Self {
        Self::False
    }
}

// This is an extension trait for "flipping" values,
// currently it is mostly a placeholder, but this is the trait
// you would use if you wanted to describe, for example,
// what the opposite of a regex.find() would look like
trait Negate {
    type Opposite;

    fn negate<T>(&self, negate: T) -> Self::Opposite
    where
        T: Into<BoolExt>;
}

impl Negate for bool {
    type Opposite = bool;

    fn negate<T>(&self, negate: T) -> bool
    where
        T: Into<BoolExt>,
    {
        match negate.into() {
            BoolExt::True => !self,
            BoolExt::False => *self,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterSeed {
    #[serde(alias = "all")]
    And(Vec<FilterSeed>),
    #[serde(alias = "any")]
    Or(Vec<FilterSeed>),
    Not(Vec<FilterSeed>),
    #[serde(alias = "re", alias = "rx", deserialize_with = "de_regex")]
    Regex(Regex),
}

#[derive(Deserialize, Debug)]
struct FWrap {
    filter: DeIntermediate,
}

type DeIntermediate = HashMap<String, Vec<FilterSeed>>;

fn de_regex<'de, D>(de: D) -> Result<Regex, D::Error>
where
    D: Deserializer<'de>,
{
    let type_hint: String = Deserialize::deserialize(de)?;

    Regex::new(&type_hint).map_err(|e| de::Error::custom(e))
}
