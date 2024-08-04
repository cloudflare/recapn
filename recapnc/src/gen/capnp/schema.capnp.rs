#![allow(unused)]

use recapnpc::prelude::gen as _p;

use super::capnp_schema_capnp as __file;

_p::generate_file! {
    struct Node {
        mod: node,
        size: { data: 5, ptrs: 6 },
        fields: {
            ID, id, id_mut, u64 = { slot: 0, default: 0 },
            DISPLAY_NAME, display_name, display_name_mut, _p::Text = { slot: 0, default: None },
            DISPLAY_NAME_PREFIX_LENGTH, display_name_prefix_length, display_name_prefix_length_mut, u32 = { slot: 2, default: 0 },
            SCOPE_ID, scope_id, scope_id_mut, u64 = { slot: 2, default: 0 },
            PARAMETERS, parameters, parameters_mut, _p::List<_p::Struct<node::Parameter>> = {
                slot: 5, default: None
            },
            IS_GENERIC, is_generic, is_generic_mut, bool = { slot: 288, default: false },
            NESTED_NODES, nested_nodes, nested_nodes_mut, _p::List<_p::Struct<node::NestedNode>> = {
                slot: 1, default: None
            },
            ANNOTATIONS, annotations, annotations_mut, _p::List<_p::Struct<Annotation>> = {
                slot: 2, default: None
            },
        },
        union {
            tag_slot: 6,
            fields: {
                FILE, File, file, file_mut, () = { case: 0 },
                STRUCT, Struct, r#struct, struct_mut, _p::Group<__file::node::Struct> = { case: 1 },
                ENUM, Enum, r#enum, enum_mut, _p::Group<__file::node::Enum> = { case: 2 },
                INTERFACE, Interface, interface, interface_mut, _p::Group<__file::node::Interface> = { case: 3 },
                CONST, Const, r#const, const_mut, _p::Group<__file::node::Const> = { case: 4 },
                ANNOTATION, Annotation, annotation, annotation_mut, _p::Group<__file::node::Annotation> = { case: 5 },
            },
        },
        nested: {
            group Struct {
                mod: r#struct,
                fields: {
                    DATA_WORD_COUNT, data_word_count, data_word_count_mut, u16 = { slot: 7, default: 0 },
                    POINTER_COUNT, pointer_count, pointer_count_mut, u16 = { slot: 12, default: 0 },
                    PREFERRED_LIST_ENCODING, preferred_list_encoding, preferred_list_encoding_mut, _p::Enum<__file::ElementSize> = {
                        slot: 13, default: __file::ElementSize::Empty
                    },
                    IS_GROUP, is_group, is_group_mut, bool = { slot: 224, default: false },
                    DISCRIMINANT_COUNT, discriminant_count, discriminant_count_mut, u16 = { slot: 15, default: 0 },
                    DISCRIMINANT_OFFSET, discriminant_offset, discriminant_offset_mut, u32 = { slot: 8, default: 0 },
                    FIELDS, fields, fields_mut, _p::List<_p::Struct<__file::Field>> = {
                        slot: 3, default: None
                    },
                },
            },
            group Enum {
                mod: r#enum,
                fields: {
                    ENUMERANTS, enumerants, enumerants_mut, _p::List<_p::Struct<__file::Enumerant>> = {
                        slot: 3, default: None
                    },
                },
            },
            group Interface {
                mod: interface,
                fields: {
                    METHODS, methods, methods_mut, _p::List<_p::Struct<__file::Method>> = {
                        slot: 3, default: None
                    },
                    SUPERCLASSES, superclasses, superclasses_mut, _p::List<_p::Struct<__file::Superclass>> = {
                        slot: 4, default: None
                    },
                },
            },
            group Const {
                mod: r#const,
                fields: {
                    TYPE, r#type, type_mut, _p::Struct<__file::Type> = { slot: 3, default: None },
                    VALUE, value, value_mut, _p::Struct<__file::Value> = { slot: 4, default: None },
                },
            },
            group Annotation {
                mod: annotation,
                fields: {
                    TYPE, r#type, type_mut, _p::Struct<__file::Type> = { slot: 3, default: None },
                    TARGETS_FILE, targets_file, targets_file_mut, bool = { slot: 112, default: false },
                    TARGETS_CONST, targets_const, targets_const_mut, bool = { slot: 113, default: false },
                    TARGETS_ENUM, targets_enum, targets_enum_mut, bool = { slot: 114, default: false },
                    TARGETS_ENUMERANT, targets_enumerant, targets_enumerant_mut, bool = { slot: 115, default: false },
                    TARGETS_STRUCT, targets_struct, targets_struct_mut, bool = { slot: 116, default: false },
                    TARGETS_FIELD, targets_field, targets_field_mut, bool = { slot: 117, default: false },
                    TARGETS_UNION, targets_union, targets_union_mut, bool = { slot: 118, default: false },
                    TARGETS_GROUP, targets_group, targets_group_mut, bool = { slot: 119, default: false },
                    TARGETS_INTERFACE, targets_interface, targets_interface_mut, bool = { slot: 120, default: false },
                    TARGETS_METHOD, targets_method, targets_method_mut, bool = { slot: 121, default: false },
                    TARGETS_PARAM, targets_param, targets_param_mut, bool = { slot: 122, default: false },
                    TARGETS_ANNOTATION, targets_annotation, targets_annotation_mut, bool = { slot: 123, default: false },
                },
            },
            struct Parameter {
                mod: parameter,
                size: { data: 0, ptrs: 1 },
                fields: {
                    NAME, name, name_mut, _p::Text = { slot: 0, default: None },
                },
            },
            struct NestedNode {
                mod: nested_node,
                size: { data: 0, ptrs: 1 },
                fields: {
                    NAME, name, name_mut, _p::Text = { slot: 0, default: None },
                    ID, id, id_mut, u64 = { slot: 0, default: 0 },
                },
            },
            struct SourceInfo {
                mod: source_info,
                size: { data: 0, ptrs: 1 },
                fields: {
                    ID, id, id_mut, u64 = { slot: 0, default: 0 },
                    DOC_COMMENT, doc_comment, doc_comment_mut, _p::Text = { slot: 0, default: None },
                    MEMBERS, members, members_mut, _p::List<_p::Struct<source_info::Member>> = {
                        slot: 1, default: None
                    },
                },
                nested: {
                    struct Member {
                        mod: member,
                        size: { data: 0, ptrs: 1 },
                        fields: {
                            DOC_COMMENT, doc_comment, doc_comment_mut, _p::Text = { slot: 0, default: None },
                        },
                    }
                },
            },
        },
    },
    struct Field {
        mod: field,
        size: { data: 3, ptrs: 4 },
        fields: {
            NAME, name, name_mut, _p::Text = { slot: 0, default: None },
            CODE_ORDER, code_order, code_order_mut, u16 = { slot: 0, default: 0 },
            ANNOTATIONS, annotations, annotations_mut, _p::List<_p::Struct<Annotation>> = {
                slot: 1, default: None
            },
            DISCRIMINANT_VALUE, discriminant_value, discriminant_value_mut, u16 = { slot: 1, default: 65535 },
            ORDINAL, ordinal, ordinal_mut, _p::Group<field::Ordinal> = (),
        },
        union {
            tag_slot: 4,
            fields: {
                SLOT, Slot, slot, slot_mut, _p::Group<__file::field::Slot> = { case: 0 },
                GROUP, Group, group, group_mut, _p::Group<__file::field::Group> = { case: 1 },
            },
        },
        nested: {
            group Slot {
                mod: slot,
                fields: {
                    OFFSET, offset, offset_mut, u32 = { slot: 1, default: 0 },
                    TYPE, r#type, type_mut, _p::Struct<__file::Type> = { slot: 2, default: None },
                    DEFAULT_VALUE, default_value, default_value_mut, _p::Struct<__file::Value> = { slot: 3, default: None },
                    HAD_EXPLICIT_DEFAULT, had_explicit_default, had_explicit_default_mut, bool = { slot: 128, default: false },
                },
            },
            group Group {
                mod: group,
                fields: {
                    TYPE_ID, type_id, type_id_mut, u64 = { slot: 2, default: 0 },
                },
            },
            group Ordinal {
                mod: ordinal,
                fields: {},
                union {
                    tag_slot: 5,
                    fields: {
                        IMPLICIT, Implicit, implicit, implicit_mut, ()  = { case: 0 },
                        EXPLICIT, Explicit, explicit, explicit_mut, u16 = { case: 1, slot: 6, default: 0 },
                    },
                },
            },
        },
    },
    struct Enumerant {
        mod: enumerant,
        size: { data: 1, ptrs: 2 },
        fields: {
            NAME, name, name_mut, _p::Text = { slot: 0, default: None },
            CODE_ORDER, code_order, code_order_mut, u16 = { slot: 0, default: 0 },
            ANNOTATIONS, annotations, annotations_mut, _p::List<_p::Struct<Annotation>> = {
                slot: 1, default: None
            },
        },
    },
    struct Superclass {
        mod: superclass,
        size: { data: 1, ptrs: 1 },
        fields: {
            ID, id, id_mut, u64 = { slot: 0, default: 0 },
            BRAND, brand, brand_mut, _p::List<_p::Struct<Brand>> = {
                slot: 0, default: None
            }
        },
    },
    struct Method {
        mod: method,
        size: { data: 3, ptrs: 5 },
        fields: {
            NAME, name, name_mut, _p::Text = { slot: 0, default: None },
            CODE_ORDER, code_order, code_order_mut, u16 = { slot: 0, default: 0 },
            IMPLICIT_PARAMETERS, implicit_parameters, implicit_parameters_mut, _p::List<_p::Struct<node::Parameter>> = {
                slot: 4, default: None
            },
            PARAM_STRUCT_TYPE, param_struct_type, param_struct_type_mut, u64 = { slot: 1, default: 0 },
            PARAM_BRAND, param_brand, param_brand_mut, _p::Struct<Brand> = { slot: 2, default: None },
            RESULT_STRUCT_TYPE, result_struct_type, result_struct_type_mut, u64 = { slot: 2, default: 0 },
            RESULT_BRAND, result_brand, result_brand_mut, _p::Struct<Brand> = { slot: 3, default: None },
            ANNOTATIONS, annotations, annotations_mut, _p::List<_p::Struct<Annotation>> = {
                slot: 1, default: None
            },
        },
    },
    struct Type {
        mod: r#type,
        size: { data: 3, ptrs: 1 },
        fields: {},
        union {
            tag_slot: 0,
            fields: {
                VOID, Void, void, void_mut, () = { case: 0 },
                BOOL, Bool, bool, bool_mut, () = { case: 1 },
                INT8, Int8, int8, int8_mut, () = { case: 2 },
                INT16, Int16, int16, int16_mut, () = { case: 3 },
                INT32, Int32, int32, int32_mut, () = { case: 4 },
                INT64, Int64, int64, int64_mut, () = { case: 5 },
                UINT8, Uint8, uint8, uint8_mut, () = { case: 6 },
                UINT16, Uint16, uint16, uint16_mut, () = { case: 7 },
                UINT32, Uint32, uint32, uint32_mut, () = { case: 8 },
                UINT64, Uint64, uint64, uint64_mut, () = { case: 9 },
                FLOAT32, Float32, float32, float32_mut, () = { case: 10 },
                FLOAT64, Float64, float64, float64_mut, () = { case: 11 },
                TEXT, Text, text, text_mut, () = { case: 12 },
                DATA, Data, data, data_mut, () = { case: 13 },
                LIST, List, list, list_mut, _p::Group<__file::r#type::List> = { case: 14 },
                ENUM, Enum, r#enum, enum_mut, _p::Group<__file::r#type::Enum> = { case: 15 },
                STRUCT, Struct, r#struct, struct_mut, _p::Group<__file::r#type::Struct> = { case: 16 },
                INTERFACE, Interface, interface, interface_mut, _p::Group<__file::r#type::Interface> = { case: 17 },
                ANY_POINTER, AnyPointer, any_pointer, any_pointer_mut, _p::Group<__file::r#type::AnyPointer> = { case: 18 },
            },
        },
        nested: {
            group List {
                mod: list,
                fields: {
                    ELEMENT_TYPE, element_type, element_type_mut, _p::Struct<__file::Type> = { slot: 0, default: None },
                },
            },
            group Enum {
                mod: r#enum,
                fields: {
                    TYPE_ID, type_id, type_id_mut, u64 = { slot: 1, default: 0 },
                    BRAND, brand, brand_mut, _p::Struct<__file::Brand> = { slot: 0, default: None },
                },
            },
            group Struct {
                mod: r#struct,
                fields: {
                    TYPE_ID, type_id, type_id_mut, u64 = { slot: 1, default: 0 },
                    BRAND, brand, brand_mut, _p::Struct<__file::Brand> = { slot: 0, default: None },
                },
            },
            group Interface {
                mod: interface,
                fields: {
                    TYPE_ID, type_id, type_id_mut, u64 = { slot: 1, default: 0 },
                    BRAND, brand, brand_mut, _p::Struct<__file::Brand> = { slot: 0, default: None },
                },
            },
            group AnyPointer {
                mod: any_pointer,
                fields: {},
                union {
                    tag_slot: 4,
                    fields: {
                        UNCONSTRAINED, Unconstrained, unconstrained, unconstrained_mut, _p::Group<__file::r#type::any_pointer::Unconstrained> = { case: 0 },
                        PARAMETER, Parameter, parameter, parameter_mut, _p::Group<__file::r#type::any_pointer::Parameter> = { case: 1 },
                        IMPLICIT_METHOD_PARAMETER, ImplicitMethodParameter, implicit_method_parameter, implicit_method_parameter_mut, _p::Group<__file::r#type::any_pointer::ImplicitMethodParameter> = { case: 2 },
                    },
                },
                nested: {
                    group Unconstrained {
                        mod: unconstrained,
                        fields: {},
                        union {
                            tag_slot: 5,
                            fields: {
                                ANY_KIND, AnyKind, any_kind, any_kind_mut, () = { case: 0 },
                                STRUCT, Struct, r#struct, r#struct_mut, () = { case: 1 },
                                LIST, List, list, list_mut, () = { case: 2 },
                                CAPABILITY, Capability, capability, capability_mut, () = { case: 3 },
                            },
                        },
                    },
                    group Parameter {
                        mod: parameter,
                        fields: {
                            SCOPE_ID, scope_id, scope_id_mut, u64 = { slot: 2, default: 0 },
                            PARAMETER_INDEX, parameter_index, parameter_index_mut, u16 = { slot: 5, default: 0 },
                        },
                    },
                    group ImplicitMethodParameter {
                        mod: implicit_method_parameter,
                        fields: {
                            PARAMETER_INDEX, parameter_index, parameter_index_mut, u16 = { slot: 5, default: 0 },
                        },
                    },
                },
            },
        },
    },
    struct Brand {
        mod: brand,
        size: { data: 0, ptrs: 1 },
        fields: {
            SCOPES, scopes, scopes_mut, _p::List<_p::Struct<__file::brand::Scope>> = {
                slot: 0, default: None
            }
        },
        nested: {
            struct Scope {
                mod: scope,
                size: { data: 2, ptrs: 1 },
                fields: {
                    SCOPE_ID, scope_id, scope_id_mut, u64 = { slot: 0, default: 0 },
                },
                union {
                    tag_slot: 4,
                    fields: {
                        BIND, Bind, bind, bind_mut, _p::List<_p::Struct<__file::brand::Binding>> = {
                            case: 0, slot: 0, default: None
                        },
                        INHERIT, Inherit, inherit, inherit_mut, () = { case: 1 },
                    },
                },
            },
            struct Binding {
                mod: binding,
                size: { data: 1, ptrs: 1 },
                fields: {},
                union {
                    tag_slot: 0,
                    fields: {
                        UNBOUND, Unbound, unbound, unbound_mut, () = { case: 0 },
                        TYPE, Type, r#type, type_mut, _p::Struct<__file::Type> = { case: 1, slot: 0, default: None },
                    },
                },
            },
        },
    },
    struct Value {
        mod: value,
        size: { data: 2, ptrs: 1 },
        fields: {},
        union {
            tag_slot: 0,
            fields: {
                VOID, Void, void, void_mut, () = { case: 0 },
                BOOL, Bool, bool, bool_mut, bool = { case: 1, slot: 16, default: false },
                INT8,  Int8,  int8,  int8_mut,  i8  = { case: 2, slot: 2, default: 0 },
                INT16, Int16, int16, int16_mut, i16 = { case: 3, slot: 1, default: 0 },
                INT32, Int32, int32, int32_mut, i32 = { case: 4, slot: 1, default: 0 },
                INT64, Int64, int64, int64_mut, i64 = { case: 5, slot: 1, default: 0 },
                UINT8,  Uint8,  uint8,  uint8_mut,  u8  = { case: 6, slot: 2, default: 0 },
                UINT16, Uint16, uint16, uint16_mut, u16 = { case: 7, slot: 1, default: 0 },
                UINT32, Uint32, uint32, uint32_mut, u32 = { case: 8, slot: 1, default: 0 },
                UINT64, Uint64, uint64, uint64_mut, u64 = { case: 9, slot: 1, default: 0 },
                FLOAT32, Float32, float32, float32_mut, f32 = { case: 10, slot: 1, default: 0. },
                FLOAT64, Float64, float64, float64_mut, f64 = { case: 11, slot: 1, default: 0. },
                TEXT, Text, text, text_mut, _p::Text = { case: 12, slot: 0, default: None },
                DATA, Data, data, data_mut, _p::Data = { case: 13, slot: 0, default: None },
                LIST, List, list, list_mut, _p::AnyPtr = { case: 14, slot: 0, default: None },
                ENUM, Enum, r#enum, enum_mut, u16 = { case: 15, slot: 1, default: 0 },
                STRUCT, Struct, r#struct, struct_mut, _p::AnyPtr = { case: 16, slot: 0, default: None },
                INTERFACE, Interface, interface, interface_mut, () = { case: 17 },
                ANY_POINTER, AnyPointer, any_pointer, any_pointer_mut, _p::AnyPtr = { case: 18, slot: 0, default: None },
            },
        },
    },
    struct Annotation {
        mod: annotation,
        size: { data: 1, ptrs: 2 },
        fields: {
            ID, id, id_mut, u64 = { slot: 0, default: 0 },
            BRAND, brand, brand_mut, _p::List<_p::Struct<Brand>> = {
                slot: 1, default: None
            },
            VALUE, value, value_mut, _p::Struct<__file::Value> = { slot: 0, default: None },
        },
    },
    struct CapnpVersion {
        mod: capnp_version,
        size: { data: 1, ptrs: 0 },
        fields: {
            MAJOR, major, major_mut, u16 = { slot: 0, default: 0 },
            MINOR, minor, minor_mut, u8 = { slot: 2, default: 0 },
            MICRO, micro, micro_mut, u8 = { slot: 3, default: 0 },
        },
    },
    struct CodeGeneratorRequest {
        mod: code_generator_request,
        size: { data: 0, ptrs: 4 },
        fields: {
            CAPNP_VERSION, capnp_version, capnp_version_mut, _p::Struct<CapnpVersion> = { slot: 2, default: None },
            NODES, nodes, nodes_mut, _p::List<_p::Struct<Node>> = {
                slot: 0, default: None
            },
            SOURCE_INFO, source_info, source_info_mut, _p::List<_p::Struct<node::SourceInfo>> = {
                slot: 3, default: None
            },
            REQUESTED_FILES, requested_files, requested_files_mut, _p::List<_p::Struct<code_generator_request::RequestedFile>> = {
                slot: 1, default: None
            }
        },
        nested: {
            struct RequestedFile {
                mod: requested_file,
                size: { data: 1, ptrs: 2 },
                fields: {
                    ID, id, id_mut, u64 = { slot: 0, default: 0 },
                    FILENAME, filename, filename_mut, _p::Text = { slot: 0, default: None },
                    IMPORTS, imports, imports_mut, _p::List<_p::Struct<__file::code_generator_request::requested_file::Import>> = {
                        slot: 1, default: None
                    },
                },
                nested: {
                    struct Import {
                        mod: import,
                        size: { data: 1, ptrs: 1 },
                        fields: {
                            ID, id, id_mut, u64 = { slot: 0, default: 0 },
                            NAME, name, name_mut, _p::Text = { slot: 0, default: None },
                        },
                    },
                },
            },
        },
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ElementSize {
    #[default]
    Empty,
    Bit,
    Byte,
    TwoBytes,
    FourBytes,
    EightBytes,
    Pointer,
    InlineComposite,
}

impl core::convert::TryFrom<u16> for ElementSize {
    type Error = _p::NotInSchema;

    #[inline]
    fn try_from(value: u16) -> Result<Self, _p::NotInSchema> {
        Ok(match value {
            0 => ElementSize::Empty,
            1 => ElementSize::Bit,
            2 => ElementSize::Byte,
            3 => ElementSize::TwoBytes,
            4 => ElementSize::FourBytes,
            5 => ElementSize::EightBytes,
            6 => ElementSize::Pointer,
            7 => ElementSize::InlineComposite,
            _ => return Err(_p::NotInSchema(value)),
        })
    }
}

impl From<ElementSize> for u16 {
    #[inline]
    fn from(value: ElementSize) -> Self {
        value as u16
    }
}

impl _p::ty::Enum for ElementSize {}
