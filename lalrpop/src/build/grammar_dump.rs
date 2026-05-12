//! Human-readable grammar dump emitted alongside generated `.rs` files
//! when `Configuration::emit_grammar(true)` is set.

use crate::grammar::parse_tree::{
    Alternative, ExprSymbol, Grammar as PtGrammar, GrammarItem, NonterminalData, Symbol,
    SymbolKind, TerminalString,
};
use crate::util::Sep;
use std::collections::BTreeSet;
use std::io::{self, Write};

pub(crate) struct GrammarDumpOptions {
    pub strip_positions: bool,
    pub strip_errors: bool,
}

pub(crate) fn write_grammar_dump<W: Write>(
    out: &mut W,
    grammar: &PtGrammar,
    opts: GrammarDumpOptions,
) -> io::Result<()> {
    // Build filtered list of (nonterminal, kept alternatives) in source order.
    let mut blocks: Vec<(&NonterminalData, Vec<&Alternative>)> = Vec::new();
    for item in &grammar.items {
        if let GrammarItem::Nonterminal(ref nt) = *item {
            let kept: Vec<&Alternative> = nt
                .alternatives
                .iter()
                .filter(|alt| !(opts.strip_errors && alternative_contains_error(alt)))
                .collect();
            if kept.is_empty() {
                continue;
            }
            blocks.push((nt, kept));
        }
    }

    for (i, (nt, alts)) in blocks.iter().enumerate() {
        if i > 0 {
            out.write_all(b"\n")?;
        }
        let name = nonterminal_header(nt);
        if alts.len() == 1 {
            let mut buf = Vec::new();
            write_alternative(&mut buf, alts[0], &opts)?;
            let s = String::from_utf8(buf).expect("alternative produced invalid UTF-8");
            let trimmed = s.trim_end();
            writeln!(out, "{name} := {trimmed}")?;
        } else {
            writeln!(out, "{name} :=")?;
            for (j, alt) in alts.iter().enumerate() {
                let mut buf = Vec::new();
                write_alternative(&mut buf, alt, &opts)?;
                let s = String::from_utf8(buf).expect("alternative produced invalid UTF-8");
                let trimmed = s.trim_end();
                if j == 0 {
                    if trimmed.is_empty() {
                        out.write_all(b"\n")?;
                    } else {
                        writeln!(out, "      {trimmed}")?;
                    }
                } else if trimmed.is_empty() {
                    writeln!(out, "    |")?;
                } else {
                    writeln!(out, "    | {trimmed}")?;
                }
            }
        }
    }

    if !blocks.is_empty() {
        out.write_all(b"\n")?;
    }
    out.write_all(b"Tokens:\n")?;
    for t in collect_terminals(grammar, &opts) {
        writeln!(out, "  {t}")?;
    }

    Ok(())
}

fn nonterminal_header(nt: &NonterminalData) -> String {
    if nt.args.is_empty() {
        format!("{}", nt.name)
    } else {
        format!("{}<{}>", nt.name, Sep(", ", &nt.args))
    }
}

fn alternative_contains_error(alt: &Alternative) -> bool {
    expr_contains_error(&alt.expr)
}

fn expr_contains_error(expr: &ExprSymbol) -> bool {
    expr.symbols.iter().any(symbol_contains_error)
}

fn symbol_contains_error(sym: &Symbol) -> bool {
    match sym.kind {
        SymbolKind::Error => true,
        SymbolKind::Terminal(TerminalString::Error) => true,
        SymbolKind::Expr(ref e) => expr_contains_error(e),
        SymbolKind::Choose(ref s) => symbol_contains_error(s),
        SymbolKind::Name(_, ref s) => symbol_contains_error(s),
        SymbolKind::Tuple(_, ref s) => symbol_contains_error(s),
        SymbolKind::Repeat(ref r) => symbol_contains_error(&r.symbol),
        SymbolKind::Macro(ref m) => m.args.iter().any(symbol_contains_error),
        SymbolKind::Terminal(_)
        | SymbolKind::Nonterminal(_)
        | SymbolKind::AmbiguousId(_)
        | SymbolKind::Lookahead
        | SymbolKind::Lookbehind => false,
    }
}

fn write_alternative<W: Write>(
    out: &mut W,
    alt: &Alternative,
    opts: &GrammarDumpOptions,
) -> io::Result<bool> {
    write_expr_symbol(out, &alt.expr, opts)
}

fn write_expr_symbol<W: Write>(
    out: &mut W,
    expr: &ExprSymbol,
    opts: &GrammarDumpOptions,
) -> io::Result<bool> {
    let mut pieces: Vec<Vec<u8>> = Vec::new();
    for sym in &expr.symbols {
        let mut buf = Vec::new();
        if write_symbol(&mut buf, sym, opts)? {
            pieces.push(buf);
        }
    }
    if pieces.is_empty() {
        return Ok(false);
    }
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            out.write_all(b" ")?;
        }
        out.write_all(piece)?;
    }
    Ok(true)
}

fn write_symbol<W: Write>(
    out: &mut W,
    sym: &Symbol,
    opts: &GrammarDumpOptions,
) -> io::Result<bool> {
    match sym.kind {
        SymbolKind::Lookahead | SymbolKind::Lookbehind if opts.strip_positions => Ok(false),
        SymbolKind::Expr(ref e) => {
            let mut inner = Vec::new();
            let wrote = write_expr_symbol(&mut inner, e, opts)?;
            if !wrote {
                return Ok(false);
            }
            out.write_all(b"(")?;
            out.write_all(&inner)?;
            out.write_all(b")")?;
            Ok(true)
        }
        SymbolKind::Choose(ref s) => write_symbol(out, s, opts),
        SymbolKind::Name(_, ref s) => write_symbol(out, s, opts),
        SymbolKind::Tuple(_, ref s) => write_symbol(out, s, opts),
        SymbolKind::Repeat(ref r) => {
            let mut inner = Vec::new();
            let wrote = write_symbol(&mut inner, &r.symbol, opts)?;
            if !wrote {
                return Ok(false);
            }
            out.write_all(&inner)?;
            write!(out, "{}", r.op)?;
            Ok(true)
        }
        SymbolKind::Macro(ref m) => {
            let mut arg_pieces: Vec<Vec<u8>> = Vec::new();
            for a in &m.args {
                let mut buf = Vec::new();
                if write_symbol(&mut buf, a, opts)? {
                    arg_pieces.push(buf);
                }
            }
            write!(out, "{}<", m.name)?;
            for (i, piece) in arg_pieces.iter().enumerate() {
                if i > 0 {
                    out.write_all(b", ")?;
                }
                out.write_all(piece)?;
            }
            out.write_all(b">")?;
            Ok(true)
        }
        _ => {
            write!(out, "{}", sym.kind)?;
            Ok(true)
        }
    }
}

fn collect_terminals(grammar: &PtGrammar, opts: &GrammarDumpOptions) -> BTreeSet<TerminalString> {
    let mut out = BTreeSet::new();
    for item in &grammar.items {
        if let GrammarItem::Nonterminal(ref nt) = *item {
            for alt in &nt.alternatives {
                if opts.strip_errors && alternative_contains_error(alt) {
                    continue;
                }
                collect_from_expr(&alt.expr, opts, &mut out);
            }
        }
    }
    out
}

fn collect_from_expr(
    expr: &ExprSymbol,
    opts: &GrammarDumpOptions,
    out: &mut BTreeSet<TerminalString>,
) {
    for sym in &expr.symbols {
        collect_from_symbol(sym, opts, out);
    }
}

fn collect_from_symbol(
    sym: &Symbol,
    opts: &GrammarDumpOptions,
    out: &mut BTreeSet<TerminalString>,
) {
    match sym.kind {
        SymbolKind::Terminal(ref t) => {
            if opts.strip_errors && matches!(t, TerminalString::Error) {
                return;
            }
            out.insert(t.clone());
        }
        SymbolKind::Expr(ref e) => collect_from_expr(e, opts, out),
        SymbolKind::Choose(ref s) => collect_from_symbol(s, opts, out),
        SymbolKind::Name(_, ref s) => collect_from_symbol(s, opts, out),
        SymbolKind::Tuple(_, ref s) => collect_from_symbol(s, opts, out),
        SymbolKind::Repeat(ref r) => collect_from_symbol(&r.symbol, opts, out),
        SymbolKind::Macro(ref m) => {
            for a in &m.args {
                collect_from_symbol(a, opts, out);
            }
        }
        SymbolKind::Nonterminal(_)
        | SymbolKind::AmbiguousId(_)
        | SymbolKind::Lookahead
        | SymbolKind::Lookbehind
        | SymbolKind::Error => {}
    }
}
