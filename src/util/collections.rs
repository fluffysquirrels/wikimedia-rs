use std::iter::Iterator;

pub trait IteratorExt<Item> {
    fn boxed(self) -> Box<dyn Iterator<Item = Item> + Send + Sync + 'static>;
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
