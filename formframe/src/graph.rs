use {
    generational_arena::{Arena, Index},
    once_cell::sync::OnceCell,
};

#[derive(Debug)]
pub struct Node<T> {
    pub datum: T,
    pub edges: OnceCell<Vec<Index>>,
}

impl<T> Node<T> {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(datum: T, arena: &mut Arena<Node<T>>) -> Index {
        arena.insert(Node {
            datum,
            edges: OnceCell::new(),
        })
    }

    pub fn new_unallocated(datum: T) -> Self {
        Self {
            datum,
            edges: OnceCell::new(),
        }
    }

    pub fn traverse_with<F, R>(&self, traverse: &F, arena: &Arena<Node<T>>) -> R
    where
        F: Fn(&Arena<Node<T>>, &T, &[Index]) -> R,
        R: Sized + Send + Sync,
    {
        traverse(arena, &self.datum, self.edges.get_or_init(Default::default))
    }
}
