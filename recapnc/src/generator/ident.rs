use std::collections::HashSet;
use std::hash::Hash;

use crate::generator::GeneratorError;
use crate::Result;

fn make_ident(s: &str) -> Result<syn::Ident> {
    let mut ident_str = make_ident_str(s)?;
    let res = syn::parse_str(&ident_str).unwrap_or_else(|_| {
        ident_str.insert_str(0, "r#");
        syn::parse_str::<syn::Ident>(&ident_str).expect("valid identifier string")
    });
    Ok(res)
}

// Convert a string into a valid identifier string by replacing invalid characters with underscores.
fn make_ident_str(s: &str) -> Result<String> {
    if s.is_empty() {
        return Err(GeneratorError::EmptyName.into());
    }

    let mut output = String::with_capacity(s.len());

    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if unicode_ident::is_xid_start(first) {
        output.push(first)
    } else {
        output.push('_')
    }

    output.extend(chars.map(|c| {
        if unicode_ident::is_xid_continue(c) {
            c
        } else {
            '_'
        }
    }));

    Ok(output)
}

pub struct IdentifierSet {
    idents: HashSet<syn::Ident>,
}

impl IdentifierSet {
    pub fn new() -> Self {
        Self {
            idents: HashSet::new(),
        }
    }

    pub fn make_unique(&mut self, name: impl ToString) -> Result<syn::Ident> {
        let mut name = name.to_string();
        let mut ident = make_ident(&name)?;
        loop {
            if self.idents.contains(&ident) {
                name.push('_');
                ident = make_ident(&name)?;
            } else {
                self.idents.insert(ident.clone());
                break Ok(ident);
            }
        }
    }

    pub fn clear(&mut self) {
        self.idents.clear()
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct Scope<T> {
    pub file: T,
    pub types: Vec<T>,
}

impl<T> Scope<T> {
    pub fn file(file: T) -> Self {
        Self {
            file,
            types: Vec::new(),
        }
    }

    pub fn push(&mut self, next: T) {
        self.types.push(next);
    }

    pub fn pop(&mut self) {
        self.types.pop();
    }

    pub fn parent(&self) -> &T {
        self.types.last().unwrap_or(&self.file)
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
struct ScopedIdent<T> {
    scope: Option<Scope<T>>,
    ident: syn::Ident,
}

pub struct ScopedIdentifierSet<T> {
    idents: HashSet<ScopedIdent<T>>,
}

impl<T: Hash + Eq> ScopedIdentifierSet<T> {
    pub fn new() -> Self {
        Self {
            idents: HashSet::new(),
        }
    }

    pub fn make_unique(
        &mut self,
        scope: Option<Scope<T>>,
        name: impl ToString,
    ) -> Result<syn::Ident> {
        let mut name = name.to_string();
        let mut scoped = ScopedIdent {
            scope,
            ident: make_ident(&name)?,
        };
        loop {
            if self.idents.contains(&scoped) {
                name.push('_');
                scoped.ident = make_ident(&name)?;
            } else {
                let ident = scoped.ident.clone();
                self.idents.insert(scoped);
                break Ok(ident);
            }
        }
    }

    pub fn clear(&mut self) {
        self.idents.clear()
    }
}
