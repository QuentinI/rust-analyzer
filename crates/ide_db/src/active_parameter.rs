//! This module provides functionality for querying callable information about a token.

use either::Either;
use hir::{Semantics, Type};
use syntax::{
    ast::{self, HasArgList, HasName},
    AstNode, SyntaxToken,
};

use crate::RootDatabase;

#[derive(Debug)]
pub struct ActiveParameter {
    pub ty: Type,
    pub pat: Either<ast::SelfParam, ast::Pat>,
}

impl ActiveParameter {
    /// Returns information about the call argument this token is part of.
    pub fn at_token(sema: &Semantics<RootDatabase>, token: SyntaxToken) -> Option<Self> {
        let (signature, active_parameter) = callable_for_token(sema, token)?;

        let idx = active_parameter?;
        let mut params = signature.params(sema.db);
        if !(idx < params.len()) {
            cov_mark::hit!(too_many_arguments);
            return None;
        }
        let (pat, ty) = params.swap_remove(idx);
        pat.map(|pat| ActiveParameter { ty, pat })
    }

    pub fn ident(&self) -> Option<ast::Name> {
        self.pat.as_ref().right().and_then(|param| match param {
            ast::Pat::IdentPat(ident) => ident.name(),
            _ => None,
        })
    }
}

/// Returns a [`hir::Callable`] this token is a part of and its argument index of said callable.
pub fn callable_for_token(
    sema: &Semantics<RootDatabase>,
    token: SyntaxToken,
) -> Option<(hir::Callable, Option<usize>)> {
    // Find the calling expression and it's NameRef
    let parent = token.parent()?;
    let calling_node = parent.ancestors().filter_map(ast::CallableExpr::cast).find(|it| {
        it.arg_list()
            .map_or(false, |it| it.syntax().text_range().contains(token.text_range().start()))
    })?;

    let callable = match &calling_node {
        ast::CallableExpr::Call(call) => {
            let expr = call.expr()?;
            sema.type_of_expr(&expr)?.adjusted().as_callable(sema.db)
        }
        ast::CallableExpr::MethodCall(call) => sema.resolve_method_call_as_callable(call),
    }?;
    let active_param = if let Some(arg_list) = calling_node.arg_list() {
        let param = arg_list
            .args()
            .take_while(|arg| arg.syntax().text_range().end() <= token.text_range().start())
            .count();
        Some(param)
    } else {
        None
    };
    Some((callable, active_param))
}

pub fn generics_for_token(
    sema: &Semantics<RootDatabase>,
    token: SyntaxToken,
) -> Option<(hir::GenericDef, usize)> {
    let parent = token.parent()?;
    let arg_list = parent
        .ancestors()
        .filter_map(ast::GenericArgList::cast)
        .find(|list| list.syntax().text_range().contains(token.text_range().start()))?;

    let active_param = arg_list
        .generic_args()
        .take_while(|arg| arg.syntax().text_range().end() <= token.text_range().start())
        .count();

    if let Some(path) = arg_list.syntax().ancestors().find_map(ast::Path::cast) {
        let res = sema.resolve_path(&path)?;
        let generic_def: hir::GenericDef = match res {
            hir::PathResolution::Def(hir::ModuleDef::Adt(it)) => it.into(),
            hir::PathResolution::Def(hir::ModuleDef::Function(it)) => it.into(),
            hir::PathResolution::Def(hir::ModuleDef::Trait(it)) => it.into(),
            hir::PathResolution::Def(hir::ModuleDef::TypeAlias(it)) => it.into(),
            hir::PathResolution::Def(hir::ModuleDef::Variant(it)) => it.into(),
            hir::PathResolution::Def(hir::ModuleDef::BuiltinType(_))
            | hir::PathResolution::Def(hir::ModuleDef::Const(_))
            | hir::PathResolution::Def(hir::ModuleDef::Macro(_))
            | hir::PathResolution::Def(hir::ModuleDef::Module(_))
            | hir::PathResolution::Def(hir::ModuleDef::Static(_)) => return None,
            hir::PathResolution::AssocItem(hir::AssocItem::Function(it)) => it.into(),
            hir::PathResolution::AssocItem(hir::AssocItem::TypeAlias(it)) => it.into(),
            hir::PathResolution::AssocItem(hir::AssocItem::Const(_)) => return None,
            hir::PathResolution::BuiltinAttr(_)
            | hir::PathResolution::ToolModule(_)
            | hir::PathResolution::Local(_)
            | hir::PathResolution::TypeParam(_)
            | hir::PathResolution::ConstParam(_)
            | hir::PathResolution::SelfType(_) => return None,
        };

        Some((generic_def, active_param))
    } else if let Some(method_call) = arg_list.syntax().parent().and_then(ast::MethodCallExpr::cast)
    {
        // recv.method::<$0>()
        let method = sema.resolve_method_call(&method_call)?;
        Some((method.into(), active_param))
    } else {
        None
    }
}
