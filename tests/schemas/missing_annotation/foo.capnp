@0xb782057765b9e46e;

# Add a test annotation, but don't use the "unused" annotation. It should be missing in the
# CodeGeneratorRequest and originally caused an error to occur in the code generator.
$import "annotate.capnp".test("hi!");

struct Foo {}

