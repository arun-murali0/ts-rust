use oxc_ast::ast::{Program, Statement};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use super::context::CheckContext;
use super::expressions::{check_excess_properties, infer_expression_type};

mod classes;
mod control_flow;
mod functions;
mod patterns;
mod support;
mod variables;

pub(super) use patterns::{bind_params, bind_pattern};
pub(super) use support::statement_always_exits;

use classes::check_class_declaration;
use control_flow::{
    check_for_statement, check_if_statement, check_switch_statement, check_while_statement,
};
use functions::check_function_declaration;
use variables::check_variable_declaration;

pub fn check_top_level(program: &Program, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) {
    for stmt in &program.body {
        check_statement(stmt, scoping, ctx);
    }
}

pub(super) fn check_return_statement(
    ret: &oxc_ast::ast::ReturnStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    // undefined for a bare `return;`, matching what a function that falls off the
    // end (or returns with no value) actually produces at runtime.
    let actual = match &ret.argument {
        Some(expr) => infer_expression_type(expr, scoping, ctx),
        None => ctx.arena.undefined(),
    };

    if let Some(expected) = ctx.current_return_type {
        if !ctx.semantic().is_assignable(actual, expected) {
            ctx.error(
                crate::diagnostic_messages::messages::return_type_mismatch(),
                ret.span(),
            );
        } else if let Some(argument) = &ret.argument {
            check_excess_properties(argument, expected, ctx);
        }
    }
}

pub(super) fn check_expression_statement(
    expr_stmt: &oxc_ast::ast::ExpressionStatement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    infer_expression_type(&expr_stmt.expression, scoping, ctx);
}

// The single dispatch point for every statement kind this checker understands,
// mirroring infer_expression_type in expressions/mod.rs. Type-only declarations
// (interfaces, aliases, enums) have already done their work in the earlier
// declare pass and need no body-checking here, so they are explicitly matched to
// a no-op rather than falling through to the unsupported-statement warning.
pub(crate) fn check_statement(stmt: &Statement, scoping: &Scoping, ctx: &mut CheckContext<'_, '_>) {
    match stmt {
        Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_) => {}
        Statement::BlockStatement(block) => {
            for inner in &block.body {
                check_statement(inner, scoping, ctx);
            }
        }
        Statement::IfStatement(if_stmt) => check_if_statement(if_stmt, scoping, ctx),
        Statement::VariableDeclaration(decl) => check_variable_declaration(decl, scoping, ctx),
        Statement::FunctionDeclaration(func) => check_function_declaration(func, scoping, ctx),
        Statement::ReturnStatement(ret) => check_return_statement(ret, scoping, ctx),
        Statement::ExpressionStatement(expr_stmt) => {
            check_expression_statement(expr_stmt, scoping, ctx)
        }
        Statement::ClassDeclaration(class) => check_class_declaration(class, scoping, ctx),
        Statement::WhileStatement(while_stmt) => check_while_statement(while_stmt, scoping, ctx),
        Statement::ForStatement(for_stmt) => check_for_statement(for_stmt, scoping, ctx),
        Statement::SwitchStatement(switch_stmt) => {
            check_switch_statement(switch_stmt, scoping, ctx)
        }
        other => support::push_unsupported(other, ctx),
    }
}
