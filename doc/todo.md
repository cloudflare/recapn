# recapn

# recapnc

- [ ] Structured module layout
* Structured module layout is a generated module layout that acts as it's own tree and can be placed anywhere in the project. All we need to support this is the root module. The root module looks something like this:

Given `a.capnp` and `sub/b.capnp` which imports `a.capnp`, generate a module:
```rust
// A mod path "." keeps the module from implicitly moving deeper
// into the filesystem.
#[path = "."]
pub mod a_capnp {
    // We reach into the root mod and re-export ourself as "__file"
    // because we can't `use self as __file` (it doesn't work).
    use super::a_capnp as __file;
    mod __imports {
    }

    #[path = "a.capnp.rs"]
    mod a_capnp;
    pub use a_capnp::*;
}

// Invalid identifier characters get replaced with '_'. So 'sub/b.capnp'
// becomes 'sub_b_capnp'.
#[path = "."]
pub mod sub_b_capnp {
    use super::sub_b_capnp as __file;
    mod __imports {
        pub use super::super::a_capnp;
    }

    #[path = "sub/b.capnp.rs"]
    mod sub_b_capnp;
    pub use sub_b_capnp::*;
}
```

- [ ] build.rs support
* We need to be able to actually call the capnp compiler to run our commands in a build.rs file.
  We can mostly copy capnp-rust for this. It's implementation is ok.

- [ ] List defaults
* I mostly just haven't finished them. It's already halfway done.

- [ ] Any pointer defaults
* Beyond just the basics (null). Uncommonly used.

- [ ] Generics
* These are a somewhat uncommonly used feature. An example of what this might look like in practice might be something like this:
```capnp
struct Map(Key, Value) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :Value;
  }
}
```

```rust
pub struct Map<Key = AnyPtr, Value = AnyPtr, T = Family> {
    key: PhantomData<fn() -> Key>,
    value: PhantomData<fn() -> Value>,
    inner: T,
}

pub mod map {
    pub struct Entry<Key = AnyPtr, Value = AnyPtr, T = Family> {
        key: PhantomData<fn() -> Key>,
        value: PhantomData<fn() -> Value>,
        inner: T,
    }
}
```

- [ ] Interface support
* I'm not exactly sure what this will end up looking like. Not at least until RPC is closer to completion.

- [ ] Annotations
* I'm also not sure how this will look until schemas are implemented in recapn, which will likely happen after RPC.

# recapn-rpc