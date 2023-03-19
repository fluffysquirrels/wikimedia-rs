use std::iter::Iterator;

pub trait IteratorExt<Item> {
    fn boxed(self) -> Box<dyn Iterator<Item = Item> + Send + Sync + 'static>;
}

pub trait IteratorExtLocal<Item> {
    fn boxed_local(self) -> Box<dyn Iterator<Item = Item> + 'static>;
}

impl<Inner, Item> IteratorExt<Item> for Inner
    where Inner: Iterator<Item = Item> + Send + Sync + 'static
{
    fn boxed(
        self
    ) -> Box<dyn Iterator<Item = Item> + Send + Sync + 'static>
    {
        Box::new(self) as Box<dyn Iterator<Item = Item> + Send + Sync + 'static>
    }
}

impl<Inner, Item> IteratorExtLocal<Item> for Inner
    where Inner: Iterator<Item = Item> + 'static
{
    fn boxed_local(
        self
    ) -> Box<dyn Iterator<Item = Item> + 'static>
    {
        Box::new(self) as Box<dyn Iterator<Item = Item> + 'static>
    }
}
