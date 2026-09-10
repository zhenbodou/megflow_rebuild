use quote::ToTokens;
use syn::{fold::Fold, visit::Visit, visit_mut::VisitMut};

#[derive(Default)]
struct Calls(Vec<String>);

impl<'ast> Visit<'ast> for Calls {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*expression.func {
            self.0.push(path.to_token_stream().to_string());
        }
        // Recurse into arguments too: outer(inner()) contains two calls.
        syn::visit::visit_expr_call(self, expression);
    }
}

struct ReplaceOne;

impl VisitMut for ReplaceOne {
    fn visit_lit_int_mut(&mut self, literal: &mut syn::LitInt) {
        // Deliberately narrow syntax rule, not arbitrary semantic refactoring.
        if literal.suffix().is_empty() && literal.base10_parse::<u64>().ok() == Some(1) {
            *literal = syn::LitInt::new("2", literal.span());
        }
    }
}

struct ReplaceTwo;

impl Fold for ReplaceTwo {
    fn fold_lit_int(&mut self, literal: syn::LitInt) -> syn::LitInt {
        if literal.suffix().is_empty() && literal.base10_parse::<u64>().ok() == Some(2) {
            syn::LitInt::new("3", literal.span())
        } else {
            literal
        }
    }
}

fn main() -> syn::Result<()> {
    let mut expression: syn::Expr = syn::parse_str("outer(inner(1), hidden!(secret(1)))")?;
    let mut calls = Calls::default();
    calls.visit_expr(&expression);
    assert_eq!(calls.0, ["outer", "inner"]);
    println!("函数调用：{}", calls.0.join(", "));

    ReplaceOne.visit_expr_mut(&mut expression);
    assert_eq!(
        expression.to_token_stream().to_string(),
        "outer (inner (2) , hidden ! (secret (1)))"
    );
    let expression = ReplaceTwo.fold_expr(expression);
    assert_eq!(
        expression.to_token_stream().to_string(),
        "outer (inner (3) , hidden ! (secret (1)))"
    );
    println!("字面量依次从 1 改为 2、3；宏内部 token 保持 1");
    Ok(())
}
