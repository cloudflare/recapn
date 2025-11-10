@0x9ad88b88dff111f1;

annotation name(struct, field, union, group, enum, enumerant, interface, method, param) :Text;

annotation modName(struct, group, interface) :Text;
# Rename the generated module associated with the given item.
#
# Note: not all items are given a module. Types without nested members will not have a module.

annotation use(file) :Text;
# Add a `use` statement to the file module with the given use tree.

annotation skip(struct, group, enum, interface, field) :Void;
# Skips generating this field or type. Generated structs will not contain this field or any
# serialization or deserialization for the field.

annotation replace(struct, group, enum, interface) :Text;
# Do not generate a port of this type. Instead, export the given type path in the use tree.

annotation with(field) :Void;
# Overrides the type of the given pointer field with another type path.

annotation attribute(struct, field, union, group, enum, enumerant, interface, method, param) :Text;
# Applies the given attribute to the generated Rust item.

annotation modAttribute(file, struct, group, interface) :Text;
# Applies the given attribute to the associated module for the generated Rust item.

annotation string(field) :Void;
# When applied to a text field, handles the field as a UTF-8 string. Note: this can make invalid
# UTF-8 fail to parse an entire struct.