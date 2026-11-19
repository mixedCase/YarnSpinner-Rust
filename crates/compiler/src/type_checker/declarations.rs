//! The compiler-internal form of [`Declaration`], which can hold unresolved types and parse tree references.

use super::types::*;
use crate::prelude::generated::yarnspinnerparser::ExpressionContextAll;
use crate::prelude::*;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) struct Decl<'input> {
    pub(crate) name: String,
    pub(crate) ty: Ty,
    pub(crate) default_value: Option<YarnValue>,
    pub(crate) description: Option<String>,
    pub(crate) source_file_name: DeclarationSource,
    pub(crate) source_node_name: Option<String>,
    pub(crate) is_implicit: bool,
    /// Whether this is a smart variable, whose expression is evaluated every time it is read.
    pub(crate) is_inline_expansion: bool,
    pub(crate) range: Option<Range<Position>>,
    /// The index of the file and the expression of the `<<declare>>` statement.
    pub(crate) initial_value: Option<(usize, Rc<ExpressionContextAll<'input>>)>,
    /// An empty diagnostic pointing at the `<<declare>>` statement.
    pub(crate) declaration_site: Option<Diagnostic>,
}

impl<'input> Decl<'input> {
    pub(crate) fn is_variable(&self) -> bool {
        !matches!(self.ty, Ty::Function(_))
    }

    /// An empty diagnostic pointing at where the variable was declared or first used.
    pub(crate) fn diagnostic(&self) -> Diagnostic {
        self.declaration_site.clone().unwrap_or_else(|| {
            let mut diagnostic = Diagnostic::from_message("");
            if let DeclarationSource::File(file_name) = &self.source_file_name {
                diagnostic = diagnostic.with_file_name(file_name);
            }
            if let Some(range) = self.range.clone() {
                diagnostic = diagnostic.with_range(range);
            }
            diagnostic
        })
    }
}

impl From<&Declaration> for Decl<'_> {
    fn from(declaration: &Declaration) -> Self {
        Self {
            name: declaration.name.clone(),
            ty: Ty::from(&declaration.r#type),
            default_value: declaration.default_value.clone(),
            description: declaration.description.clone(),
            source_file_name: declaration.source_file_name.clone(),
            source_node_name: declaration.source_node_name.clone(),
            is_implicit: declaration.is_implicit,
            is_inline_expansion: declaration.is_inline_expansion,
            range: declaration.range.clone(),
            initial_value: None,
            declaration_site: None,
        }
    }
}

impl From<&Decl<'_>> for Declaration {
    fn from(decl: &Decl<'_>) -> Self {
        Self {
            name: decl.name.clone(),
            default_value: decl.default_value.clone(),
            description: decl.description.clone(),
            source_file_name: decl.source_file_name.clone(),
            source_node_name: decl.source_node_name.clone(),
            is_implicit: decl.is_implicit,
            is_inline_expansion: decl.is_inline_expansion,
            r#type: decl.ty.to_type(),
            range: decl.range.clone(),
        }
    }
}

/// All declarations known to a compilation, in the order they were added. Lookups by name find the first one.
#[derive(Clone, Default)]
pub(crate) struct Declarations<'input> {
    // Removed declarations leave a hole so that the indices in `lookup` stay valid.
    list: Vec<Option<Decl<'input>>>,
    lookup: HashMap<String, usize>,
}

impl<'input> Declarations<'input> {
    pub(crate) fn add(&mut self, declaration: Decl<'input>) {
        self.lookup
            .entry(declaration.name.clone())
            .or_insert(self.list.len());
        self.list.push(Some(declaration));
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Decl<'input>> {
        self.list[*self.lookup.get(name)?].as_ref()
    }

    pub(crate) fn remove(&mut self, name: &str) {
        if let Some(index) = self.lookup.remove(name) {
            self.list[index] = None;
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Decl<'input>> {
        self.list.iter().flatten()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Decl<'input>> {
        self.list.iter_mut().flatten()
    }
}
