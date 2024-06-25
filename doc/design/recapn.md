# recapn

'recapn' is a **re**implementation of **Cap'n** Proto from the ground up. Rather than follow the C++
implementation line-for-line, word-for-word, this library aims to take a step back and consider the
Rust language when choosing how to design parts of the library.

The primary goal is mostly better ergonomics, but also better support for things such as no-std,
no-alloc, and Cap'n Proto features like orphans, schemas, and dynamic types.

# Overview

In this section I want to go over the overall design of each module, what it contains, and why it
differs from the original capnp-rust implementation. Starting from lib.rs, I'll go over each module
in the order they're defined, which I intended to order based on the modules being "layers" of
components in the library. But first, we need to talk about a common pattern heavily used by this
library.

## Multi-impl Type Wrap Pattern

For context: most of the time, types in Cap'n Proto are basically just sugar over some other
primitive types. They essentially act as type-checking interpreters over dynamic data. There are
really only 3 different types of "things" in the Cap'n Proto protocol: structs, lists, and 
capabilities. The different types that are generated based on a user's schema are just interpreters
of one of these things in the message.

In C++, these type wrappers have their own separate types nested inside another type. These types
are named `Foo::Reader`, `Foo::Builder`, and `Foo::Pipeline`. Obviously in Rust we can't really
effectively replicate this. Rust does not have nested types so this inherently won't work.

capnp-rust attempts to work around this by using a module to encapsulate separate "Reader",
"Builder", and "Pipeline" types. It also names an "Owned" type used for generic programming. This
isn't without it's own set of issues. For one, importing all these types isn't great. You're
basically stuck importing modules and then referencing the type you want off the module like
`foo::Reader`. Types with multiple words just don't look great like `foo_bar::Builder`. Inline
annotations are effectively non-functional because they're based on the type not the type and the
module the type is in. So you see a lot of `let x: Reader<'_>` type stuff when you use
rust-analyzer.

Recognizing that types are effectively just wrappers, recapn types look something like this:

```rust
pub struct Foo<T = Family>(T);

impl Struct for Foo {
    type Reader<'a> = Foo<StructReader<'a>>;
    type Builder<'b> = Foo<StructBuilder<'a>>;
}

impl Foo<StructReader<'_>> {
    pub fn bar(&self) -> i32 {
        /* do thing */
    }
}

impl Foo<StructBuilder<'_>> {
    pub fn bar(&self) -> i32 {
        /* do thing */
    }

    pub fn set_bar(&mut self, value: i32) {
        /* do thing */
    }
}
```

I don't know what to call this pattern because I don't see it too often so I call it the multi-impl
type wrap.

The idea is simple: a type that's generic can represent any kind of derivative wrapper type for the
type `T` and the type itself (`Foo`) for metaprogramming. That is, through generics and
non-overlapping implementations, a `Foo<T>` can be a `Foo<TypeA>` used to represent the type itself,
a `Foo<TypeB>` as a wrapper of `TypeB`, and a `Foo<TypeC>` as a wrapper of `TypeC`.

In the context of recapn a `Foo` or `Foo<Family>` can implement traits related to using the type
`Foo` for metaprogramming. For example, a `Struct` trait with associated types `Reader` and
`Builder`. We can then use these along with type aliasing to make it so we don't have to ever name a
reader type specifically. We can define a `ReaderOf<T>` alias like so:

```rust
pub type ReaderOf<'a, T> = <T as StructView>::Reader<'a>;
```

And then we can reference a struct type's reader as `ReaderOf<Foo>`.

`Foo<StructReader>`, or in this case `ReaderOf<Foo>`, can implement traits and methods related to a
reader of `Foo`. If `Foo` has a `Int32` `bar` field we could define a `bar()` function in the
implementation of `Foo<StructReader<'_>>` to get the field value.

`Foo<StructBuilder>`, or `BuilderOf<Foo>`, can implement traits and methods the same way, but for
builder methods that modify the struct in the message. It can even name functions that are the same
as in the reader implementation. The implementations don't overlap, so you can have
`ReaderOf<Foo>::bar()` and `BuilderOf<Foo>::bar()` that do different things!

This surprisingly has many benefits. For one, you only really need to import a type once to access
it's readers and builders. It's easier to use these types with generic programming, and it's even
*recommended* since it's extremely clean and easy to read. These types are resolvable by
rust-analyzer and are actually understandable since they show up as `Foo<StructReader>`.

This pattern also extends to basically everything else in the library! `List(Int32)` in recapn is
represented as a `List<Int32, T>`. Field and list element accessors are highly generic and
implemented entirely in the library. The only thing you see in generated code is
`fn bar(&self) -> Accessor<Int32>` or `fn bar(&mut self) -> AccessorMut<Int32>`.

So now that we understand this crucial pattern used in the library, we can move on to module design.

## `lib.rs` and error philosphy

The root mod mostly contains error types. capnp-rust error handling follows the C++ implementation
by defining a single error type with a "type" and a heap allocated "description" string. Then a year
ago the no-alloc devs got mad and now it's a mish-mash of C++ design and every single possible error
the library can produce as separate enum variants.

In recapn we don't do this. Somewhat. We at least don't do it as horribly. Separate error types are
defined for different modules in the library, clearly marking API boundaries and keeping scope
limited. This makes it easier to handle different errors related to specific modules of the library.

In lib.rs, `Error` and `ErrorKind` are exclusively used by serialization and deserialization parts
of the library. `NotInSchema` is used when reading enums. In the `ptr` module, a `FailedRead` type
represents all failure conditions when reading a pointer. It contains some "expected" pointer type
and an "actual" read pointer type instead of an infinite assortment of
`MessageContainsNonCapabilityPointerWhereCapabilityPointerWasExpected` error variants.
`IncompatibleUpgrade` is similarly used to represent cases where a list type conversion fails and is
exposed separately for certain utility functions and also prevents infinite
`FoundBitListWhereStructListWasExpected` error variants.

The `io` modules have many of their own error types specific to their module and sometimes specific
functions which might only be enabled with the std feature.

We do not use `thiserror` to define error types since it is [not compatible with no-std](https://github.com/dtolnay/thiserror/issues/196#issuecomment-1636066787).

## `num`

This contains simple reduced-bit integer types. Specifically 29 bit unsigned integers (`u29`) and
30 bit signed integers (`i30`). These types are largely used in pointer related code. I didn't want
to pull in an external crate for this since these types are

 * Relatively simple, they only have two operations: "new" to check and wrap and "get" to unwrap
 * External crates might have some extraneous features that make them incompatible with no-std.

Other than that this module is very straightforward.

## `alloc`

Contains structs relating to how message segments are allocated. These are reusable allocator
features that can be combined to create flexible allocator types for the default message type.

In C++ you create a message type by subclassing the `MessageBuilder` type and implementing the
`allocateSegment` function. The implementation of this that's provided and that you're expected to
use is the `MallocMessageBuilder`. This allocator

* Allocates from malloc.
* Will either increase the size of segments allocated by a "multiplicative factor", or keep the
  sizes fixed unless required.
* Supports providing scratch space from some source, like the stack, for messages where you are
  confident it's some specific size.

This obviously does not work in Rust because it has no subclassing. capnp-rust attempts to follow
the design of C++ in a more Rusty way by having a `Builder` type that's generic over an allocator
type. This allocator type implements the `Allocator` trait which has functions to

* `allocate_segment`, which returns the segment size and a... byte pointer...
* and `deallocate_segment` to deallocate it.

capnp-rust tries really hard to follow C++ with it's allocator implementations. It has 3 types

* `HeapAllocator` which does everything `MallocMessageBuilder` does except support scratch space
* `ScratchSpaceHeapAllocator` which does everything `MallocMessageBuilder` does.
* `SingleSegmentAllocator` which only supports scratch space and then panics if you try and do
  anything else.

As you can see there is a lot of duplicate logic between the types. It might be better to split the
different features of the allocators into separate types so that these features can be reused.

This is what I opted to do in recapn. Each feature of these allocators is a distinct allocator type.
You can combine these features to create whatever allocator system you want. There are 5 allocators
based on the types provided by capnp-rust:

* `Global` which just allocates a segment from the global allocator.
* `Never` which never allocates.
* `Fixed` which attempts to allocate all segments with the same size.
* `Growing` which implements the increasing size feature.
* `Scratch` which implements allocation from some "space".

Making these allocators reusable features enhances the extensibility of the library. You could add
logging or metrics to the global allocator and not have to redefine all the other features you're
using.