//! Diagnostic codes of rusty-cargo (prefix R). The codes are fixed identifiers; each is chosen at an explicit match
//! arm below, keyed on rustc's own error code (E0308), or on the lint name (unused_variables, clippy::len_zero),
//! never on message wording. The one exception is a syntax error: rustc gives parse errors no code at all, so those
//! are recognised by the message start. Anything not listed gets the generic code of its severity.
//!
//!   R0001 unresolved name        R0002 syntax error           R0003 unresolved import or missing crate
//!   R0004 type mismatch          R0005 trait or method problem (bound not satisfied, no method, ambiguity)
//!   R0006 ownership and borrowing   R0007 lifetime problem    R0008 wrong number or kind of arguments or generics
//!   R0009 unused or dead code    R0010 deprecated             R0011 clippy lint
//!   R0012 privacy                R0013 missing items (match arms, trait items, fields)
//!   R0014 linker or build script failure     R0015 other rustc lint
//!   fallbacks: R0100 error, R0200 warning, R0300 fatal
use crate::diag::Sev;

pub fn classify(sev: Sev, rustc_code: Option<&str>, flag: Option<&str>, msg: &str, has_span: bool) -> &'static str {
    if let Some(e) = rustc_code {
        match e {
            "E0425" | "E0412" | "E0422" | "E0423" | "E0424" | "E0426" | "E0405" | "E0408" | "E0531" | "E0532" | "E0574" | "E0433" => return "R0001",
            "E0432" | "E0463" | "E0583" => return "R0003",
            "E0308" | "E0053" | "E0271" | "E0605" | "E0606" | "E0607" | "E0604" | "E0620" | "E0069" | "E0070" => return "R0004",
            "E0277" | "E0599" | "E0282" | "E0283" | "E0284" | "E0369" | "E0368" | "E0600" | "E0608" | "E0609" | "E0610" | "E0614" | "E0615"
            | "E0618" | "E0592" | "E0119" | "E0117" | "E0038" | "E0034" | "E0191" | "E0220" | "E0221" | "E0195" | "E0200" | "E0201" => return "R0005",
            "E0381" | "E0382" | "E0384" | "E0499" | "E0502" | "E0503" | "E0505" | "E0506" | "E0507" | "E0508" | "E0509" | "E0510" | "E0594"
            | "E0596" | "E0373" | "E0521" | "E0524" | "E0712" | "E0713" => return "R0006",
            "E0106" | "E0261" | "E0262" | "E0263" | "E0309" | "E0310" | "E0495" | "E0515" | "E0597" | "E0621" | "E0623" | "E0700" | "E0716"
            | "E0759" => return "R0007",
            "E0061" | "E0057" | "E0060" | "E0050" | "E0107" | "E0087" | "E0089" | "E0090" | "E0023" | "E0026" | "E0027" | "E0029" => return "R0008",
            "E0603" | "E0616" | "E0624" | "E0451" | "E0446" | "E0364" | "E0365" => return "R0012",
            "E0004" | "E0005" | "E0046" | "E0063" | "E0062" | "E0124" | "E0449" | "E0317" | "E0072" => return "R0013",
            _ => {}
        }
    }
    if let Some(f) = flag {
        if f.starts_with("clippy::") { return "R0011"; }
        match f {
            "unused_imports" | "unused_variables" | "unused_mut" | "unused_assignments" | "dead_code" | "unused_must_use" | "unused_parens"
            | "unused_braces" | "unused_macros" | "unused_unsafe" | "unused_attributes" | "unused_doc_comments" | "unused_labels"
            | "unused_results" | "unused_features" | "unused_extern_crates" | "unused_lifetimes" | "unused_qualifications"
            | "unused_import_braces" | "unreachable_code" | "unreachable_patterns" | "unused_comparisons" | "unused_allocation"
            | "unused_macro_rules" | "unused_crate_dependencies" | "path_statements" => return "R0009",
            "deprecated" | "deprecated_in_future" | "deprecated_safe" | "deprecated_where_clause_location" => return "R0010",
            _ => return "R0015",
        }
    }
    if rustc_code.is_none() && flag.is_none() && has_span && sev != Sev::Warning {
        if msg.starts_with("expected ") || msg.starts_with("unexpected ") || msg.starts_with("unclosed delimiter")
            || msg.starts_with("mismatched closing delimiter") || msg.starts_with("this file contains an unclosed delimiter")
            || msg.starts_with("unknown start of token") || msg.starts_with("missing ") { return "R0002"; }
    }
    if !has_span && msg.starts_with("linking with") { return "R0014"; }
    match sev { Sev::Fatal => "R0300", Sev::Error => "R0100", _ => "R0200" }
}

