mod atom;

use super::*;

pub(crate) use self::atom::{block_expr, match_arm_list};
pub(super) use self::atom::{literal, LITERAL_FIRST};

pub(super) enum StmtWithSemi {
    Yes,
    No,
    Optional,
}

const EXPR_FIRST: TokenSet = LHS_FIRST;

pub(super) fn expr(p: &mut Parser) -> bool {
    let r = Restrictions { forbid_structs: false, prefer_stmt: false };
    expr_bp(p, None, r, 1).is_some()
}

pub(super) fn expr_stmt(p: &mut Parser, m: Option<Marker>) -> Option<(CompletedMarker, BlockLike)> {
    let r = Restrictions { forbid_structs: false, prefer_stmt: true };
    expr_bp(p, m, r, 1)
}

fn expr_no_struct(p: &mut Parser) {
    let r = Restrictions { forbid_structs: true, prefer_stmt: false };
    expr_bp(p, None, r, 1);
}

pub(super) fn stmt(p: &mut Parser, with_semi: StmtWithSemi, prefer_expr: bool) {
    let m = p.start();
    // test attr_on_expr_stmt
    // fn foo() {
    //     #[A] foo();
    //     #[B] bar!{}
    //     #[C] #[D] {}
    //     #[D] return ();
    // }
    attributes::outer_attrs(p);

    if p.at(T![let]) {
        let_stmt(p, m, with_semi);
        return;
    }

    // test block_items
    // fn a() { fn b() {} }
    let m = match items::opt_item(p, m) {
        Ok(()) => return,
        Err(m) => m,
    };

    if let Some((cm, blocklike)) = expr_stmt(p, Some(m)) {
        if !(p.at(T!['}']) || (prefer_expr && p.at(EOF))) {
            // test no_semi_after_block
            // fn foo() {
            //     if true {}
            //     loop {}
            //     match () {}
            //     while true {}
            //     for _ in () {}
            //     {}
            //     {}
            //     macro_rules! test {
            //          () => {}
            //     }
            //     test!{}
            // }
            let m = cm.precede(p);
            match with_semi {
                StmtWithSemi::No => (),
                StmtWithSemi::Optional => {
                    p.eat(T![;]);
                }
                StmtWithSemi::Yes => {
                    if blocklike.is_block() {
                        p.eat(T![;]);
                    } else {
                        p.expect(T![;]);
                    }
                }
            }

            m.complete(p, EXPR_STMT);
        }
    }

    // test let_stmt
    // fn f() { let x: i32 = 92; }
    fn let_stmt(p: &mut Parser, m: Marker, with_semi: StmtWithSemi) {
        p.bump(T![let]);
        patterns::pattern(p);
        if p.at(T![:]) {
            // test let_stmt_ascription
            // fn f() { let x: i32; }
            types::ascription(p);
        }
        if p.eat(T![=]) {
            // test let_stmt_init
            // fn f() { let x = 92; }
            expressions::expr(p);
        }

        match with_semi {
            StmtWithSemi::No => (),
            StmtWithSemi::Optional => {
                p.eat(T![;]);
            }
            StmtWithSemi::Yes => {
                p.expect(T![;]);
            }
        }
        m.complete(p, LET_STMT);
    }
}

pub(super) fn expr_block_contents(p: &mut Parser) {
    attributes::inner_attrs(p);

    while !p.at(EOF) && !p.at(T!['}']) {
        // test nocontentexpr
        // fn foo(){
        //     ;;;some_expr();;;;{;;;};;;;Ok(())
        // }

        // test nocontentexpr_after_item
        // fn simple_function() {
        //     enum LocalEnum {
        //         One,
        //         Two,
        //     };
        //     fn f() {};
        //     struct S {};
        // }

        if p.at(T![;]) {
            p.bump(T![;]);
            continue;
        }

        stmt(p, StmtWithSemi::Yes, false);
    }
}

#[derive(Clone, Copy)]
struct Restrictions {
    forbid_structs: bool,
    prefer_stmt: bool,
}

/// Binding powers of operators for a Pratt parser.
///
/// See <https://matklad.github.io/2020/04/13/simple-but-powerful-pratt-parsing.html>
#[rustfmt::skip]
fn current_op(p: &Parser) -> (u8, SyntaxKind) {
    const NOT_AN_OP: (u8, SyntaxKind) = (0, T![@]);
    match p.current() {
        T![|] if p.at(T![||])  => (3,  T![||]),
        T![|] if p.at(T![|=])  => (1,  T![|=]),
        T![|]                  => (6,  T![|]),
        T![>] if p.at(T![>>=]) => (1,  T![>>=]),
        T![>] if p.at(T![>>])  => (9,  T![>>]),
        T![>] if p.at(T![>=])  => (5,  T![>=]),
        T![>]                  => (5,  T![>]),
        T![=] if p.at(T![=>])  => NOT_AN_OP,
        T![=] if p.at(T![==])  => (5,  T![==]),
        T![=]                  => (1,  T![=]),
        T![<] if p.at(T![<=])  => (5,  T![<=]),
        T![<] if p.at(T![<<=]) => (1,  T![<<=]),
        T![<] if p.at(T![<<])  => (9,  T![<<]),
        T![<]                  => (5,  T![<]),
        T![+] if p.at(T![+=])  => (1,  T![+=]),
        T![+]                  => (10, T![+]),
        T![^] if p.at(T![^=])  => (1,  T![^=]),
        T![^]                  => (7,  T![^]),
        T![%] if p.at(T![%=])  => (1,  T![%=]),
        T![%]                  => (11, T![%]),
        T![&] if p.at(T![&=])  => (1,  T![&=]),
        T![&] if p.at(T![&&])  => (4,  T![&&]),
        T![&]                  => (8,  T![&]),
        T![/] if p.at(T![/=])  => (1,  T![/=]),
        T![/]                  => (11, T![/]),
        T![*] if p.at(T![*=])  => (1,  T![*=]),
        T![*]                  => (11, T![*]),
        T![.] if p.at(T![..=]) => (2,  T![..=]),
        T![.] if p.at(T![..])  => (2,  T![..]),
        T![!] if p.at(T![!=])  => (5,  T![!=]),
        T![-] if p.at(T![-=])  => (1,  T![-=]),
        T![-]                  => (10, T![-]),
        T![as]                 => (12, T![as]),

        _                      => NOT_AN_OP
    }
}

// Parses expression with binding power of at least bp.
fn expr_bp(
    p: &mut Parser,
    m: Option<Marker>,
    mut r: Restrictions,
    bp: u8,
) -> Option<(CompletedMarker, BlockLike)> {
    let m = m.unwrap_or_else(|| {
        let m = p.start();
        attributes::outer_attrs(p);
        m
    });
    let mut lhs = match lhs(p, r) {
        Some((lhs, blocklike)) => {
            let lhs = lhs.extend_to(p, m);
            if r.prefer_stmt && blocklike.is_block() {
                // test stmt_bin_expr_ambiguity
                // fn f() {
                //     let _ = {1} & 2;
                //     {1} &2;
                // }
                return Some((lhs, BlockLike::Block));
            }
            lhs
        }
        None => {
            m.abandon(p);
            return None;
        }
    };

    loop {
        let is_range = p.at(T![..]) || p.at(T![..=]);
        let (op_bp, op) = current_op(p);
        if op_bp < bp {
            break;
        }
        // test as_precedence
        // fn f() { let _ = &1 as *const i32; }
        if p.at(T![as]) {
            lhs = cast_expr(p, lhs);
            continue;
        }
        let m = lhs.precede(p);
        p.bump(op);

        // test binop_resets_statementness
        // fn f() { v = {1}&2; }
        r = Restrictions { prefer_stmt: false, ..r };

        if is_range {
            // test postfix_range
            // fn foo() {
            //     let x = 1..;
            //     match 1.. { _ => () };
            //     match a.b()..S { _ => () };
            // }
            let has_trailing_expression =
                p.at_ts(EXPR_FIRST) && !(r.forbid_structs && p.at(T!['{']));
            if !has_trailing_expression {
                // no RHS
                lhs = m.complete(p, RANGE_EXPR);
                break;
            }
        }

        expr_bp(p, None, Restrictions { prefer_stmt: false, ..r }, op_bp + 1);
        lhs = m.complete(p, if is_range { RANGE_EXPR } else { BIN_EXPR });
    }
    Some((lhs, BlockLike::NotBlock))
}

const LHS_FIRST: TokenSet =
    atom::ATOM_EXPR_FIRST.union(TokenSet::new(&[T![&], T![*], T![!], T![.], T![-]]));

fn lhs(p: &mut Parser, r: Restrictions) -> Option<(CompletedMarker, BlockLike)> {
    let m;
    let kind = match p.current() {
        // test ref_expr
        // fn foo() {
        //     // reference operator
        //     let _ = &1;
        //     let _ = &mut &f();
        //     let _ = &raw;
        //     let _ = &raw.0;
        //     // raw reference operator
        //     let _ = &raw mut foo;
        //     let _ = &raw const foo;
        // }
        T![&] => {
            m = p.start();
            p.bump(T![&]);
            if p.at(IDENT)
                && p.at_contextual_kw("raw")
                && (p.nth_at(1, T![mut]) || p.nth_at(1, T![const]))
            {
                p.bump_remap(T![raw]);
                p.bump_any();
            } else {
                p.eat(T![mut]);
            }
            REF_EXPR
        }
        // test unary_expr
        // fn foo() {
        //     **&1;
        //     !!true;
        //     --1;
        // }
        T![*] | T![!] | T![-] => {
            m = p.start();
            p.bump_any();
            PREFIX_EXPR
        }
        _ => {
            // test full_range_expr
            // fn foo() { xs[..]; }
            for op in [T![..=], T![..]] {
                if p.at(op) {
                    m = p.start();
                    p.bump(op);
                    if p.at_ts(EXPR_FIRST) && !(r.forbid_structs && p.at(T!['{'])) {
                        expr_bp(p, None, r, 2);
                    }
                    let cm = m.complete(p, RANGE_EXPR);
                    return Some((cm, BlockLike::NotBlock));
                }
            }

            // test expression_after_block
            // fn foo() {
            //    let mut p = F{x: 5};
            //    {p}.x = 10;
            // }
            let (lhs, blocklike) = atom::atom_expr(p, r)?;
            let (cm, block_like) =
                postfix_expr(p, lhs, blocklike, !(r.prefer_stmt && blocklike.is_block()));
            return Some((cm, block_like));
        }
    };
    // parse the interior of the unary expression
    expr_bp(p, None, r, 255);
    let cm = m.complete(p, kind);
    Some((cm, BlockLike::NotBlock))
}

fn postfix_expr(
    p: &mut Parser,
    mut lhs: CompletedMarker,
    // Calls are disallowed if the type is a block and we prefer statements because the call cannot be disambiguated from a tuple
    // E.g. `while true {break}();` is parsed as
    // `while true {break}; ();`
    mut block_like: BlockLike,
    mut allow_calls: bool,
) -> (CompletedMarker, BlockLike) {
    loop {
        lhs = match p.current() {
            // test stmt_postfix_expr_ambiguity
            // fn foo() {
            //     match () {
            //         _ => {}
            //         () => {}
            //         [] => {}
            //     }
            // }
            T!['('] if allow_calls => call_expr(p, lhs),
            T!['['] if allow_calls => index_expr(p, lhs),
            T![.] => match postfix_dot_expr(p, lhs) {
                Ok(it) => it,
                Err(it) => {
                    lhs = it;
                    break;
                }
            },
            T![?] => try_expr(p, lhs),
            _ => break,
        };
        allow_calls = true;
        block_like = BlockLike::NotBlock;
    }
    return (lhs, block_like);

    fn postfix_dot_expr(
        p: &mut Parser,
        lhs: CompletedMarker,
    ) -> Result<CompletedMarker, CompletedMarker> {
        assert!(p.at(T![.]));
        if p.nth(1) == IDENT && (p.nth(2) == T!['('] || p.nth_at(2, T![::])) {
            return Ok(method_call_expr(p, lhs));
        }

        // test await_expr
        // fn foo() {
        //     x.await;
        //     x.0.await;
        //     x.0().await?.hello();
        // }
        if p.nth(1) == T![await] {
            let m = lhs.precede(p);
            p.bump(T![.]);
            p.bump(T![await]);
            return Ok(m.complete(p, AWAIT_EXPR));
        }

        if p.at(T![..=]) || p.at(T![..]) {
            return Err(lhs);
        }

        Ok(field_expr(p, lhs))
    }
}

// test call_expr
// fn foo() {
//     let _ = f();
//     let _ = f()(1)(1, 2,);
//     let _ = f(<Foo>::func());
//     f(<Foo as Trait>::func());
// }
fn call_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T!['(']));
    let m = lhs.precede(p);
    arg_list(p);
    m.complete(p, CALL_EXPR)
}

// test index_expr
// fn foo() {
//     x[1][2];
// }
fn index_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T!['[']));
    let m = lhs.precede(p);
    p.bump(T!['[']);
    expr(p);
    p.expect(T![']']);
    m.complete(p, INDEX_EXPR)
}

// test method_call_expr
// fn foo() {
//     x.foo();
//     y.bar::<T>(1, 2,);
// }
fn method_call_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![.]) && p.nth(1) == IDENT && (p.nth(2) == T!['('] || p.nth_at(2, T![::])));
    let m = lhs.precede(p);
    p.bump_any();
    name_ref(p);
    generic_args::opt_generic_arg_list(p, true);
    if p.at(T!['(']) {
        arg_list(p);
    }
    m.complete(p, METHOD_CALL_EXPR)
}

// test field_expr
// fn foo() {
//     x.foo;
//     x.0.bar;
//     x.0();
// }

// test_err bad_tuple_index_expr
// fn foo() {
//     x.0.;
//     x.1i32;
//     x.0x01;
// }
fn field_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![.]));
    let m = lhs.precede(p);
    p.bump(T![.]);
    if p.at(IDENT) || p.at(INT_NUMBER) {
        name_ref_or_index(p);
    } else if p.at(FLOAT_NUMBER) {
        // FIXME: How to recover and instead parse INT + T![.]?
        p.bump_any();
    } else {
        p.error("expected field name or number");
    }
    m.complete(p, FIELD_EXPR)
}

// test try_expr
// fn foo() {
//     x?;
// }
fn try_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![?]));
    let m = lhs.precede(p);
    p.bump(T![?]);
    m.complete(p, TRY_EXPR)
}

// test cast_expr
// fn foo() {
//     82 as i32;
//     81 as i8 + 1;
//     79 as i16 - 1;
//     0x36 as u8 <= 0x37;
// }
fn cast_expr(p: &mut Parser, lhs: CompletedMarker) -> CompletedMarker {
    assert!(p.at(T![as]));
    let m = lhs.precede(p);
    p.bump(T![as]);
    // Use type_no_bounds(), because cast expressions are not
    // allowed to have bounds.
    types::type_no_bounds(p);
    m.complete(p, CAST_EXPR)
}

fn arg_list(p: &mut Parser) {
    assert!(p.at(T!['(']));
    let m = p.start();
    p.bump(T!['(']);
    while !p.at(T![')']) && !p.at(EOF) {
        // test arg_with_attr
        // fn main() {
        //     foo(#[attr] 92)
        // }
        if !expr(p) {
            break;
        }
        if !p.at(T![')']) && !p.expect(T![,]) {
            break;
        }
    }
    p.eat(T![')']);
    m.complete(p, ARG_LIST);
}

// test path_expr
// fn foo() {
//     let _ = a;
//     let _ = a::b;
//     let _ = ::a::<b>;
//     let _ = format!();
// }
fn path_expr(p: &mut Parser, r: Restrictions) -> (CompletedMarker, BlockLike) {
    assert!(paths::is_path_start(p));
    let m = p.start();
    paths::expr_path(p);
    match p.current() {
        T!['{'] if !r.forbid_structs => {
            record_expr_field_list(p);
            (m.complete(p, RECORD_EXPR), BlockLike::NotBlock)
        }
        T![!] if !p.at(T![!=]) => {
            let block_like = items::macro_call_after_excl(p);
            (m.complete(p, MACRO_CALL), block_like)
        }
        _ => (m.complete(p, PATH_EXPR), BlockLike::NotBlock),
    }
}

// test record_lit
// fn foo() {
//     S {};
//     S { x, y: 32, };
//     S { x, y: 32, ..Default::default() };
//     TupleStruct { 0: 1 };
// }
pub(crate) fn record_expr_field_list(p: &mut Parser) {
    assert!(p.at(T!['{']));
    let m = p.start();
    p.bump(T!['{']);
    while !p.at(EOF) && !p.at(T!['}']) {
        let m = p.start();
        // test record_literal_field_with_attr
        // fn main() {
        //     S { #[cfg(test)] field: 1 }
        // }
        attributes::outer_attrs(p);

        match p.current() {
            IDENT | INT_NUMBER => {
                // test_err record_literal_before_ellipsis_recovery
                // fn main() {
                //     S { field ..S::default() }
                // }
                if p.nth_at(1, T![:]) || p.nth_at(1, T![..]) {
                    name_ref_or_index(p);
                    p.expect(T![:]);
                }
                expr(p);
                m.complete(p, RECORD_EXPR_FIELD);
            }
            T![.] if p.at(T![..]) => {
                m.abandon(p);
                p.bump(T![..]);
                expr(p);
            }
            T!['{'] => {
                error_block(p, "expected a field");
                m.abandon(p);
            }
            _ => {
                p.err_and_bump("expected identifier");
                m.abandon(p);
            }
        }
        if !p.at(T!['}']) {
            p.expect(T![,]);
        }
    }
    p.expect(T!['}']);
    m.complete(p, RECORD_EXPR_FIELD_LIST);
}
