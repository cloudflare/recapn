use std::cell::UnsafeCell;
use std::convert::TryFrom;
use std::marker::PhantomData;

use super::client::Client;

type TableVec = Vec<Option<Client>>;

/// A cap table represented internally as a `Vec` of `Client`s.
/// 
/// Much like the Message type, this serves to satisfy the thread-safety type system
/// by having the type that owns the table separate from the types that mutate and read
/// the table. This makes it safe for us to move the table itself between threads, reference
/// it across threads, etc.
/// 
/// However, the reader and builder types for the table are not thread safe, even though the
/// underlying table itself is. You can't share a TableReader across threads because you might've
/// gotten it from a TableBuilder, and TableBuilders can exist alongside readers, due to the
/// requirements of recapn.
pub struct Table(UnsafeCell<TableVec>);

impl Table {
    pub fn new(vec: TableVec) -> Self {
        Self(UnsafeCell::new(vec))
    }

    pub fn reader(&self) -> TableReader<'_> {
        TableReader(&self.0)
    }

    pub fn builder(&mut self) -> TableBuilder<'_> {
        TableBuilder(&self.0)
    }
}

unsafe impl Send for Table {}
unsafe impl Sync for Table {}

/// A marker type to implement the CapTable trait over.
pub struct CapTable<'a> {
    a: PhantomData<&'a ()>,
}

impl<'a> recapn::rpc::CapSystem for CapTable<'a> {
    type Cap = Client;
}

impl<'a> recapn::rpc::CapTable for CapTable<'a> {
    type TableReader = TableReader<'a>;
    type TableBuilder = TableBuilder<'a>;
}

#[derive(Clone, Copy)]
pub struct TableReader<'a>(&'a UnsafeCell<TableVec>);

impl<'a> recapn::rpc::CapTableReader for TableReader<'a> {
    type System = CapTable<'a>;

    fn extract_cap(&self, index: u32) -> Option<Client> {
        let vec = unsafe { &*self.0.get() };
        vec.get(index as usize).cloned().flatten()
    }
}

#[derive(Clone, Copy)]
pub struct TableBuilder<'a>(&'a UnsafeCell<TableVec>);

impl<'a> recapn::rpc::CapTableReader for TableBuilder<'a> {
    type System = CapTable<'a>;

    fn extract_cap(&self, index: u32) -> Option<Client> {
        let vec = unsafe { &*self.0.get() };
        vec.get(index as usize).cloned().flatten()
    }
}

impl<'a> recapn::rpc::CapTableBuilder for TableBuilder<'a> {
    fn inject_cap(&self, cap: Client) -> Option<u32> {
        let vec = unsafe { &mut *self.0.get() };
        let next_id = u32::try_from(vec.len()).expect("too many capabilities in table");
        vec.push(Some(cap));
        Some(next_id)
    }

    fn drop_cap(&self, index: u32) {
        let vec = unsafe { &mut *self.0.get() };
        if let Some(client) = vec.get_mut(index as usize) {
            *client = None;
        }
        // TODO(someday): Return an error on invalid cap in message for validation?
    }

    fn as_reader(&self) -> TableReader<'a> {
        TableReader(self.0)
    }
}