//! Provides most of the types that ultimately get turned into quoted code.
//! 
//! This seperation makes it really easy to identify what's needed to generate
//! everything related to some item like a struct, field, enum, etc.
//! 
//! If you want to add something to generated code, add it here first to figure out
//! the code layout and identify what parameters you'll need, then modify the generator
//! to provide it.

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use recapn::ptr::StructSize;

/// A simple macro that automatically writes a ToTokens implementation based on a function
/// that returns a TokenStream
macro_rules! to_tokens {
    ($ty:ty |$self:ident| $block:block) => {
        impl ToTokens for $ty {
            fn to_token_stream($self: &Self) -> TokenStream $block

            fn to_tokens(&self, tokens: &mut TokenStream) {
                tokens.extend(self.to_token_stream())
            }

            fn into_token_stream(self) -> TokenStream
                where
                    Self: Sized,
            {
                self.to_token_stream()
            }
        }
    };
}

pub struct GeneratedFile {
    pub ident: syn::Ident,
    pub items: Vec<GeneratedItem>,
}

to_tokens!(GeneratedFile |self| {
    let Self { items, .. } = self;
    quote! {
        #![allow(unused, unsafe_code)]

        use recapn::prelude::gen as _p;
        use super::{__file, __imports};

        #(#items)*
    }
});

pub enum GeneratedItem {
    Struct(GeneratedStruct),
    Enum(GeneratedEnum),
    Const(GeneratedConst),
}

impl ToTokens for GeneratedItem {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            GeneratedItem::Struct(s) => s.to_tokens(tokens),
            GeneratedItem::Enum(e) => e.to_tokens(tokens),
            GeneratedItem::Const(c) => c.to_tokens(tokens),
        }
    }
}

pub struct GeneratedStruct {
    pub ident: syn::Ident,
    pub mod_ident: syn::Ident,
    pub type_params: Vec<syn::TypeParam>,
    pub size: Option<StructSize>,
    pub fields: Vec<GeneratedField>,
    pub which: Option<GeneratedWhich>,
    pub nested_items: Vec<GeneratedItem>,
}

impl GeneratedStruct {
    pub fn descriptors(&self) -> impl Iterator<Item = TokenStream> + '_ {
        let fields = self.fields.iter().map(GeneratedField::descriptor);
        let variants = self.which.as_ref().map(GeneratedWhich::descriptors).into_iter().flatten();
        fields.chain(variants)
    }
    pub fn ref_accessors(&self) -> impl Iterator<Item = TokenStream> + '_ {
        let fields = self.fields.iter().map(GeneratedField::ref_accessor);
        let variants = self.which.as_ref().map(GeneratedWhich::ref_accessors).into_iter().flatten();
        fields.chain(variants)
    }
    pub fn mut_accessors(&self) -> impl Iterator<Item = TokenStream> + '_ {
        let fields = self.fields.iter().map(GeneratedField::mut_accessor);
        let variants = self.which.as_ref().map(GeneratedWhich::mut_accessors).into_iter().flatten();
        fields.chain(variants)
    }
    pub fn which_accessors(&self) -> Option<(TokenStream, TokenStream)> {
        let _ = self.which.as_ref()?;
        let modname = &self.mod_ident;
        let ref_accessor = quote! {
            #[inline]
            pub fn which(&self) -> Result<#modname::Which<&Self>, _p::NotInSchema> {
                unsafe { <#modname::Which<_> as _p::UnionViewer<_>>::get(self) }
            }
        };
        let mut_accessor = quote! {
            #[inline]
            pub fn which(&mut self) -> Result<#modname::Which<&mut Self>, _p::NotInSchema> {
                unsafe { <#modname::Which<_> as _p::UnionViewer<_>>::get(self) }
            }
        };
        Some((ref_accessor, mut_accessor))
    }
    pub fn which_type(&self) -> Option<TokenStream> {
        let Self { ident: struct_name, which, .. } = self;
        let GeneratedWhich { tag_slot, fields, type_params, } = which.as_ref()?;
        let tag_slot = *tag_slot as usize;
        let disciminants = fields.iter()
            .map(|GeneratedVariant {
                discriminant_ident: name,
                discriminant_field_type: field_type,
                ..
            }| quote!(#name(_p::ViewOf<T, #field_type>)));
        let disciminant_ref_matches = fields.iter()
            .map(|GeneratedVariant {
                discriminant_ident: name,
                discriminant_field_type: field_type,
                field: GeneratedField { descriptor_ident, .. },
                case,
            }| {
                quote!(#case => Ok(Which::#name(<#field_type as _p::field::FieldType>::reader(&repr.0, &super::#struct_name::#descriptor_ident.field))))
            });
        let disciminant_mut_matches = fields.iter()
            .map(|GeneratedVariant {
                discriminant_ident: name,
                discriminant_field_type: field_type,
                field: GeneratedField { descriptor_ident, .. },
                case,
            }| {
                quote!(#case => Ok(Which::#name(<#field_type as _p::field::FieldType>::builder(&mut repr.0, &super::#struct_name::#descriptor_ident.field))))
            });

        Some(quote! {
            pub enum Which<#(#type_params,)* T: _p::Viewable = _p::Family> {
                #(#disciminants),*
            }

            impl<'b, 'p: 'b, #(#type_params,)* T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>> for Which<#(#type_params,)*> {
                type View = Which<#(#type_params,)* &'b Reader<'p, T>>;

                unsafe fn get(repr: &'b Reader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
                    let tag = repr.0.data_field::<u16>(#tag_slot);
                    match tag {
                        #(#disciminant_ref_matches,)*
                        unknown => Err(_p::NotInSchema(unknown)),
                    }
                }
            }

            impl<'b, 'p: 'b, #(#type_params,)* T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>> for Which<#(#type_params,)*> {
                type View = Which<#(#type_params,)* &'b mut Builder<'p, T>>;

                unsafe fn get(repr: &'b mut Builder<'p, T>) -> Result<Self::View, _p::NotInSchema> {
                    let tag = repr.0.data_field::<u16>(#tag_slot);
                    match tag {
                        #(#disciminant_mut_matches,)*
                        unknown => Err(_p::NotInSchema(unknown)),
                    }
                }
            }
        })
    }
}

to_tokens!(GeneratedStruct |self| {
    let Self { ident: name, mod_ident: modname, nested_items: nested, size, .. } = self;
    let descriptors = self.descriptors();
    let ref_accessors = self.ref_accessors();
    let mut_accessors = self.mut_accessors();
    let (which_ref_accessor, which_mut_accessor) = self.which_accessors().unzip();
    let which_type = self.which_type();
    let struct_view = quote! {
        impl _p::StructView for #name {
            type Reader<'a, T: _p::rpc::Table> = #modname::Reader<'a, T>;
            type Builder<'a, T: _p::rpc::Table> = #modname::Builder<'a, T>;
        }
    };
    let struct_group_marker = if let Some(StructSize { data, ptrs }) = size {
        quote! {
            impl _p::ty::Struct for #name {
                const SIZE: _p::StructSize = _p::StructSize { data: #data, ptrs: #ptrs };
            }
        }
    } else {
        quote! {
            impl _p::FieldGroup for #name {}
        }
    };
    quote! {
        #[derive(Clone)]
        pub struct #name<T = _p::Family>(T);

        impl<T> _p::IntoFamily for #name<T> {
            type Family = #name;
        }

        impl<T: _p::Capable> _p::Capable for #name<T> {
            type Table = T::Table;

            type Imbued = T::Imbued;
            type ImbuedWith<T2: _p::rpc::Table> = #name<T::ImbuedWith<T2>>;

            #[inline]
            fn imbued(&self) -> &Self::Imbued { self.0.imbued() }

            #[inline]
            fn imbue_release<T2: _p::rpc::Table>(
                self,
                new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
            ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
                let (imbued, old) = self.0.imbue_release(new_table);
                (#name(imbued), old)
            }

            #[inline]
            fn imbue_release_into<U>(
                &self,
                other: U,
            ) -> (U::ImbuedWith<Self::Table>, U::Imbued)
            where
                U: _p::Capable,
                U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
            {
                self.0.imbue_release_into(other)
            }
        }

        impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for #modname::Reader<'a, T> {
            type Ptr = _p::StructReader<'a, T>;
        }

        impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>> for #modname::Reader<'a, T> {
            #[inline]
            fn from(ptr: _p::StructReader<'a, T>) -> Self {
                #name(ptr)
            }
        }

        impl<'a, T: _p::rpc::Table> core::convert::From<#modname::Reader<'a, T>> for _p::StructReader<'a, T> {
            #[inline]
            fn from(reader: #modname::Reader<'a, T>) -> Self {
                reader.0
            }
        }

        impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>> for #modname::Reader<'a, T> {
            #[inline]
            fn as_ref(&self) -> &_p::StructReader<'a, T> {
                &self.0
            }
        }

        impl<'a, T: _p::rpc::Table> _p::ty::StructReader for #modname::Reader<'a, T> {}

        impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for #modname::Builder<'a, T> {
            type Ptr = _p::StructBuilder<'a, T>;
        }

        impl<'a, T: _p::rpc::Table> core::convert::From<#modname::Builder<'a, T>> for _p::StructBuilder<'a, T> {
            #[inline]
            fn from(reader: #modname::Builder<'a, T>) -> Self {
                reader.0
            }
        }

        impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>> for #modname::Builder<'a, T> {
            #[inline]
            fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
                &self.0
            }
        }

        impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>> for #modname::Builder<'a, T> {
            #[inline]
            fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
                &mut self.0
            }
        }

        impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for #modname::Builder<'a, T> {
            unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
                Self(ptr)
            }
        }

        #struct_view
        #struct_group_marker

        impl #name {
            #(#descriptors)*
        }

        impl<'p, T: _p::rpc::Table + 'p> #modname::Reader<'p, T> {
            #(#ref_accessors)*
            #which_ref_accessor
        }

        impl<'p, T: _p::rpc::Table + 'p> #modname::Builder<'p, T> {
            #(#mut_accessors)*
            #which_mut_accessor
        }

        pub mod #modname {
            use super::{__file, _p};

            pub type Reader<'a, T = _p::rpc::Empty> = super::#name<_p::StructReader<'a, T>>;
            pub type Builder<'a, T = _p::rpc::Empty> = super::#name<_p::StructBuilder<'a, T>>;

            #which_type

            #(#nested)*
        }
    }
});

pub struct GeneratedField {
    pub type_name: syn::Ident,
    pub field_type: Box<syn::Type>,
    pub accessor_ident: syn::Ident,
    pub descriptor_ident: syn::Ident,
    pub descriptor: Option<FieldDescriptor>,
}

pub struct FieldDescriptor {
    pub slot: u32,
    pub default: Box<syn::Expr>,
}

impl GeneratedField {
    fn descriptor_value(&self) -> TokenStream {
        let field_type = &self.field_type;
        self.descriptor
            .as_ref()
            .map(|FieldDescriptor { slot, default } | {
                quote!(_p::Descriptor::<#field_type> { 
                    slot: #slot,
                    default: #default,
                })
            })
            .unwrap_or(quote!(()))
    }

    pub fn descriptor(&self) -> TokenStream {
        let Self { descriptor_ident: name, field_type, .. } = self;
        let descriptor_value = self.descriptor_value();

        quote!(const #name: _p::Descriptor<#field_type> = #descriptor_value;)
    }

    pub fn ref_accessor(&self) -> TokenStream {
        let Self {
            accessor_ident: name,
            field_type,
            type_name,
            descriptor_ident: descriptor,
            ..
        } = self;
        quote! {
            #[inline]
            pub fn #name(&self) -> _p::Accessor<'_, 'p, T, #field_type> {
                unsafe { <#field_type as _p::field::FieldType>::reader(&self.0, &#type_name::#descriptor) }
            }
        }
    }

    pub fn mut_accessor(&self) -> TokenStream {
        let GeneratedField {
            accessor_ident: name,
            field_type,
            type_name,
            descriptor_ident: descriptor,
            ..
        } = self;
        quote! {
            #[inline]
            pub fn #name(&mut self) -> _p::AccessorMut<'_, 'p, T, #field_type> {
                unsafe { <#field_type as _p::field::FieldType>::builder(&mut self.0, &#type_name::#descriptor) }
            }
        }
    }
}

pub struct GeneratedWhich {
    pub tag_slot: u32,
    pub type_params: Vec<syn::TypeParam>,
    pub fields: Vec<GeneratedVariant>,
}

impl GeneratedWhich {
    pub fn descriptors(&self) -> impl Iterator<Item = TokenStream> + '_ {
        self.fields.iter().map(move |variant| variant.descriptor(self.tag_slot))
    }
    pub fn ref_accessors(&self) -> impl Iterator<Item = TokenStream> + '_ {
        self.fields.iter().map(move |variant| variant.ref_accessor())
    }
    pub fn mut_accessors(&self) -> impl Iterator<Item = TokenStream> + '_ {
        self.fields.iter().map(move |variant| variant.mut_accessor())
    }
}

pub struct GeneratedVariant {
    pub discriminant_ident: syn::Ident,
    /// The field type as written in the disciminant as `ViewOf<T, Type>`. This might be different
    /// from `GeneratedField::field_type` since the path resolution is different, as the Which type
    /// is defined in the type's module instead of the parent module.
    /// 
    /// For example, given a struct A with a field that refers to union group B, field and accessors
    /// in the impls of A will refer to group B via `a::B`. However, since the Which type is defined
    /// in the same place as B, it _doesn't_ refer to it with the `a` mod in the path. Instead
    /// it refers to it directly with `B`.
    pub discriminant_field_type: Box<syn::Type>,
    pub case: u16,
    pub field: GeneratedField,
}

impl GeneratedVariant {
    pub fn descriptor(&self, tag_slot: u32) -> TokenStream {
        let Self {
            case,
            field: field @ GeneratedField {
                descriptor_ident: name,
                field_type,
                ..
            },
            ..
        } = self;
        let descriptor_value = field.descriptor_value();

        quote! {
            const #name: _p::VariantDescriptor<#field_type>
                = _p::VariantDescriptor::<#field_type> {
                    variant: _p::VariantInfo {
                        slot: #tag_slot,
                        case: #case,
                    },
                    field: #descriptor_value,
                };
        }
    }
    pub fn ref_accessor(&self) -> TokenStream {
        let GeneratedField {
            accessor_ident: name,
            field_type,
            type_name,
            descriptor_ident: descriptor,
            ..
        } = &self.field;
        quote! {
            #[inline]
            pub fn #name(&self) -> _p::Variant<'_, 'p, T, #field_type> {
                unsafe { _p::field::VariantField::new(&self.0, &#type_name::#descriptor) }
            }
        }
    }

    pub fn mut_accessor(&self) -> TokenStream {
        let GeneratedField {
            accessor_ident: name,
            field_type,
            type_name,
            descriptor_ident: descriptor,
            ..
        } = &self.field;
        quote! {
            #[inline]
            pub fn #name(&mut self) -> _p::VariantMut<'_, 'p, T, #field_type> {
                unsafe { _p::field::VariantField::new(&mut self.0, &#type_name::#descriptor) }
            }
        }
    }
}

pub struct GeneratedEnum {
    pub name: syn::Ident,
    pub enumerants: Vec<syn::Ident>,
}

to_tokens!(GeneratedEnum |self| {
    let (name, enumerants) = (&self.name, &self.enumerants);
    let enum_matches = self.enumerants.iter().enumerate().map(|(value, ident)| {
        let value = value as u16;
        quote!(#value => Ok(Self::#ident))
    });

    quote! {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
        pub enum #name {
            #[default]
            #(#enumerants),*
        }

        impl core::convert::TryFrom<u16> for #name {
            type Error = _p::NotInSchema;

            #[inline]
            fn try_from(value: u16) -> Result<Self, Self::Error> {
                match value {
                    #(#enum_matches,)*
                    value => Err(_p::NotInSchema(value))
                }
            }
        }

        impl core::convert::From<#name> for u16 {
            #[inline]
            fn from(value: #name) -> Self {
                value as u16
            }
        }

        impl _p::ty::Enum for #name {}
    }
});

pub struct GeneratedConst {
    pub ident: syn::Ident,
    pub const_type: Box<syn::Type>,
    pub value: Box<syn::Expr>,
}

to_tokens!(GeneratedConst |self| {
    let Self { ident, const_type, value } = self;
    quote!(pub const #ident: #const_type = #value;)
});
