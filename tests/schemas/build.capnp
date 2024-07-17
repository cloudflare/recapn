@0xc966fd35ae09a963;

using Bar = import "bar.capnp".Bar;

struct Foo {
    bar @0 :Bar;
}