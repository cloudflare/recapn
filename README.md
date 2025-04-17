# recapn

`recapn` is a Cap'n Proto implementation for Rust. The goal of `recapn` is to provide a more modern implementation of Cap'n Proto using the latest and greatest features of Rust with a more flexible but simplified API.

# Example

```rust
pub mod generated {
    //! Auto-generated code module!
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}

use gen::addessbook_capnp::*;

let mut message = recapn::message::Message::global();
let mut builder = message.builder();
let mut addressbook = builder.by_ref().init_struct_root::<AddressBook>();

let mut people = addressbook.people().init(2);

let mut alice = people.at(0).get();
alice.id().set(123);
alice.name().set(text!("Alice"));
alice.email().set(text!("alice@example.com"));
alice.employment().school().set(text!("MIT"));

let mut alice_phones = alice.phones().init(1);
let mut phone = alice_phones.at(0).get();
phone.number().set(text!("555-1212"));
phone.r#type().set(person::phone_number::Type::Mobile);

let mut bob = people.at(1).get();
bob.id().set(456);
bob.name().set(text!("Bob"));
bob.email().set(text!("bob@example.com"));
bob.employment().unemployed().set();

let mut bob_phones = bob.phones().init(2);

let mut home = bob_phones.at(0).get();
home.number().set(text!("555-4567"));
home.r#type().set(person::phone_number::Type::Home);

let mut work = bob_phones.at(1).get();
work.number().set(text!("555-7654"));
work.r#type().set(person::phone_number::Type::Work);

recapn::io::write_message_packed(target, &builder.segments()).unwrap();
```

# Getting started

Add `recapn` to your crate dependencies.

Code is generated using `recapnc`, the capnpc compiler plugin. This can be done with either a `build.rs` script or by manually invoking `capnp` with a compiled version of the plugin. Generated code automatically includes a module that properly includes all generated modules so you don't have to manually include each generated file.

### With build.rs

Add `recapnc` to your crate build dependencies.

This example assumes you're putting all your schemas in a `schemas` directory alongside your `src` in the form:

```
project/
  - schemas/
  - src/
  - build.rs
  - Cargo.toml
```

```rust
fn main() {
    // Tell cargo to rerun the build script if our schemas change.
    println!("cargo::rerun-if-changed=schemas");

    recapnc::CapnpCommand::new()
        .src_prefix("schemas")
        .file("schemas/addressbook.capnp")
        .write_to_out_dir();
}
```

This generated code can be imported using `include!` from anywhere in the module tree, there is no dependence on the module being imported from a specific location.

```rust
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
```

And that's it! You're now ready to use `recapn`.

# Using `recapn`

### Building a message

All Cap'n Proto usage starts with a message which contains the root structure. In `recapn` you'll generally use the `Message` type for this purpose. `Message` is parameterized over an allocator that determines how segments of words are allocated in the message. You can provide your own allocator by supplying a type that implements the `recapn::alloc::Alloc` trait. The `alloc` module provides a few allocator utilities for building flexible allocators. These can be mixed and matched to build your own allocator types!

The default recommended allocator is a `Growing<Global>` allocator which is provided via the `Message::global` function. If you know your message is really small, you can also use stack allocated scratch space with the default allocator by using `Message::with_scratch`:

```rust
let mut space = recapn::alloc::space::<16>();
let mut message = Message::with_scratch(&mut space);
```

Messages will not allocate until you create a builder for the message. They can also be sent and shared between threads as long as the underlying allocator is `Send`.

Now you need to build your message. `Message::builder` creates a `recapn::message::Builder` that provides factory functions for setting up the message. The standard way to set up the message is with `init_struct_root`.

```rust
let mut builder = message.builder();
let mut addressbook = builder.by_ref().init_struct_root::<AddressBook>();
```

All of the factory functions consume the builder. You can use `Builder::by_ref` to make sure the builder isn't consumed. This is useful if you're immediately going to get the segments of the message via `Builder::segments`.

### Generated structs

As you may have noticed, generated structs look a little bit peculiar...

```rust
#[derive(Clone)]
pub struct AddressBook<T = _p::Family>(T);
```

They're all generic! `recapn` does not generate separate structs for readers and builders. Instead, all types are wrappers for `StructReader` and `StructBuilder`.

But what about naming the types? You'd think you might have to use `AddressBook<StructReader<'a>>` but the library provides alternative aliases `recapn::ReaderOf` and `recapn::BuilderOf` that can be used in the form `ReaderOf<'a, AddressBook>` or `BuilderOf<'a, AddressBook>`.

Each struct also has a corresponding module in named snake_case with `Reader` and `Builder` type aliases. This module also contains all the nested types contained within the struct's scope.

### Accessing fields

Unlike other Cap'n Proto libraries, fields in `recapn` only have one accessor per field. This accessor uses the field's name and is the same for readers and builders. The returned value from the accessor gives you the available options for reading or building the field.

Reading a simple data field like an Int32 or Bool will return the value itself.

```capnp
struct Foo {
    bar @0 :Int32;
    baz @1 :Bool;
}
```

```rust
assert_eq!(foo.bar(), 5);
assert_eq!(foo.baz(), false);
```

While building a data field will give `get()` and `set()` functions.

```rust
foo.bar().set(10);
foo.baz().set(true);

assert_eq!(foo.bar().get(), 10);
assert_eq!(foo.baz().get(), true);
```

Pointer fields (structs, lists, capabilities, text, data, etc) are more complicated, but that's just because there's more options available to use! Pointers can be null, but they can also be invalid, so other libraries expose two accessors: `has_field()` which returns a bool indicating if the field is set and `get_field()` which returns the field value wrapped in a `Result` to indicate any errors that occur. This leaves a lot to be desired, so `recapn` doesn't use this approach.

Instead, there are 6 base pointer read accessors:

 * `ptr()` which returns the field as `AnyPtr`. Useful in some edge cases.
 * `is_null()` which returns whether the field is null.
 * `get()` which returns the set value or the field default if the field is null or invalid.
 * `get_option()` which returns the set value or `None` if the field is null or invalid.
 * `try_get()` which returns the set value or the default if null. If the field is invalid, it returns an `Err`.
 * `try_get_option()` which returns the set value, an `Err` if invalid, or `None` if the field is null.

Different fields may have different accessors, so explore what's available! Choose the accessor that works for your use-case. If you're confident your data is correct you can use infallible accessors like `foo.bar().get()`. If you want validation use accessors that can return validation errors like `foo.bar().try_get()`.

# Serializing your message

Now you have a message. You want to send it somewhere! `recapn::message::Builder` provides a function `segments` to get the `MessageSegments` of your `Message`! You can pass these segments to `recapn::io::write_message` to serialize the message to a `Write`able value. This includes a stream table, allowing you to read the message even if the message contains multiple segments.

Want to customize how you write messages? Everything about serialization is exposed through `recapn::io::stream` so you can make your own stream table for writing to your own streams!

Packing support is exposed through `recapn::io::write_message_packed`. Packing primitives can be used in your own streams by using the `recapn::io::packing` module.

```rust
let mut builder = message.builder();
let mut addressbook = builder.by_ref().init_struct_root::<AddressBook>();

/* doing stuff */

let mut buf = Vec::new();
recapn::io::write_message(&mut buf, builder.segments())?;
```

# Deserializing messages

Message reading functions are similarly found in `recapn::io`. To read from a `std::io::Read` instance, use `read_from_stream`. To read from a packed stream, use `read_packed_from_stream`. Construct a `recapn::message::Reader` using the message and start reading using `Reader::read_as_struct`.

```rust
let message = recapn::io::read_from_stream(buf.as_slice(), Default::default())?;
let reader = recapn::message::Reader::new(&message);
let addressbook = reader.read_as_struct::<AddressBook>();
```

And that's the basics of using `recapn`! Currently, this library only supports the basics but over time we hope to support more features and reach feature parity with the existing Rust and C++ implementations.