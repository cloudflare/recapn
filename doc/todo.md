# recapn

- [ ] Testing (by module)
  - [ ] message
  - [ ] io
    - [ ] stream
    * Fairly confident this already works. The code generator uses it and that works! But could use some tests either way.
    - [ ] packing
    * Desparately needs testing. I wrote it to prove the API would work and then moved on to RPC so I actually have no idea if it works. Should be easier to test than one based on some streaming API.
  - [ ] ptr
  * I've already implemented a few tests for this that check a lot of stuff in the library, but it probably needs more.
  - [ ] data
  * These can be implemented through doc tests
  - [ ] text
  * Same as data, can be implemented through doc tests
  - [ ] list
  * We need to test the different patterns that are expected to be usable by users of the library.

- [ ] Field accessors! Because someone 5 years ago thought it'd be a good idea to implement them in the library.
  - [ ] `PtrVariantOwner` generic accessors
  * Needs `by_ref`, `init_field`, `field`, `is_null`, `adopt`, `disown_into`, `try_clear`, and `clear` methods. We can basically just copy `PtrVariantBuilder` for this.
  - [ ] `PtrFieldBuilder` for `List<AnyStruct>`
  * Needs `try_set`, `set`, and `try_init`
  - [ ] `PtrFieldOwner` for `List<V>`
  * Copy from `PtrFieldBuilder`. Change lifetimes as appropriate.
  - [ ] `PtrVariantBuilder` for `List<V>`
  * Needs `get`, `try_set`, `set`, `init`, `try_init`, `init_with_size`. Essentially just the same accessors as `PtrFieldBuilder` but based on the variant helpers.
  * We also need the accessors from `List<AnyStruct>` which is slightly different due to the lack of fixed size to check.
  - [ ] `PtrVariantOwner` for `List<V>`
  * Copy from `PtrVariantBuilder`.
  - [ ] `PtrFieldOwner` and `PtrVariantOwner` for `Struct<S>`
  - [ ] `PtrVariantReader` and`PtrVariantBuilder` for `Text`
  - [ ] `PtrVariantReader` and `PtrVariantBuilder` for `Data`
  - [ ] `PtrVariantReader`, `PtrVariantBuilder`, and `PtrVariantOwner` for `AnyPtr`
  * Probably just needs an alias or two for the obvious ops.
  - [ ] `PtrFieldBuilder`, `PtrFieldOwner`, `PtrVariantReader`, `PtrVariantBuilder`, and `PtrVariantOwner` for `AnyStruct`
  - [ ] `PtrFieldBuilder`, `PtrFieldOwner`, `PtrVariantReader`, `PtrVariantBuilder`, and `PtrVariantOwner` for `AnyList`
  - [ ] `PtrFieldReader`, `PtrFieldBuilder`, `PtrVariantReader`, and `PtrVariantBuilder` for `Capability<C>`
  - [ ] Support "any" capability. We need some sort of marker for the type. Other than that it just returns whatever the `CapSystem`'s `Cap` type is.
- [ ] List accessors! Same thing as field accessors, but for list elements!
 - As a whole, list accessors need to be refactored to be more inline with field accessors in their current state. This brings with it better reusability of ptr element accessors like `is_null`, `clear`, `adopt`, and `disown_into`.
 - [ ] Owning list accessors. So you can pass an element back with ownership. Useful for struct lists and list lists.

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