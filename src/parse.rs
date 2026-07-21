use std::collections::HashMap;

use scraper::{Html, Selector};

/// Extra facts about an option, taken from its "detailed description" entry:
/// `<p><a class="anchor" id=".."></a><b>name</b> <em>(integer)</em>: ...</p>`
/// followed by a `<blockquote>` that may hold a `value`/`meaning` table.
///
/// Not every GAMS solver page has these entries (BARON, CBC, COPT, HiGHS, SCIP,
/// SHOT and SoPlex publish only the summary table), so both fields are optional.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    /// Declared type: `integer`, `real`, `boolean` or `string`. Authoritative,
    /// unlike the guess made from the default value.
    pub declared_type: Option<String>,
    /// Allowed values when the option enumerates over strings. Numeric-coded
    /// enumerations are deliberately left out: their `meaning` cells are
    /// sentences, which make poor identifiers.
    pub string_values: Vec<String>,
}

/// A value is usable as an enum variant if it is a non-numeric identifier-ish
/// token (numeric codes like `0`/`1` are rejected: see [`Detail::string_values`]).
fn is_enum_token(v: &str) -> bool {
    !v.is_empty()
        && v.parse::<f64>().is_err()
        && v.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Parse the per-option "detailed description" entries into a map keyed by the
/// exact option name.
pub fn parse_option_details(html: &str) -> HashMap<String, Detail> {
    let document = Html::parse_document(html);
    let p_sel = Selector::parse("p").unwrap();
    let anchor_sel = Selector::parse("a.anchor").unwrap();
    let b_sel = Selector::parse("b").unwrap();
    let em_sel = Selector::parse("em").unwrap();
    let table_sel = Selector::parse("table.markdownTable").unwrap();
    let th_sel = Selector::parse("th").unwrap();
    let tr_sel = Selector::parse("tr").unwrap();
    let td_sel = Selector::parse("td").unwrap();

    let mut out: HashMap<String, Detail> = HashMap::new();

    for p in document.select(&p_sel) {
        if p.select(&anchor_sel).next().is_none() {
            continue;
        }
        let Some(name_el) = p.select(&b_sel).next() else {
            continue;
        };
        let name = cell_text(&name_el);
        if name.is_empty() {
            continue;
        }

        let declared_type = p.select(&em_sel).next().map(|e| cell_text(&e)).and_then(|t| {
            let t = t.trim().trim_start_matches('(').trim_end_matches(')').to_string();
            matches!(t.as_str(), "integer" | "real" | "boolean" | "string").then_some(t)
        });

        let mut values: Vec<String> = Vec::new();
        if let Some(bq) = p
            .next_siblings()
            .filter_map(scraper::ElementRef::wrap)
            .find(|e| e.value().name() == "blockquote")
        {
            for table in bq.select(&table_sel) {
                let heads: Vec<String> =
                    table.select(&th_sel).map(|th| cell_text(&th).to_lowercase()).collect();
                if !(heads.iter().any(|h| h == "value") && heads.iter().any(|h| h == "meaning")) {
                    continue;
                }
                for row in table.select(&tr_sel) {
                    if row.select(&th_sel).count() > 0 {
                        continue;
                    }
                    if let Some(cell) = row.select(&td_sel).next() {
                        let v = cell_text(&cell);
                        if !v.is_empty() {
                            values.push(v);
                        }
                    }
                }
            }
        }
        let string_values =
            if values.len() >= 2 && values.iter().all(|v| is_enum_token(v)) { values } else { Vec::new() };

        let entry = out.entry(name).or_default();
        if entry.declared_type.is_none() {
            entry.declared_type = declared_type;
        }
        if entry.string_values.is_empty() {
            entry.string_values = string_values;
        }
    }

    out
}

/// Pages without detailed entries (HiGHS, SCIP, ...) document a string option's
/// alternatives inline in its description, e.g.
/// `LP solver: "choose", "simplex", "ipm", ...` alongside `Range: string`.
/// Pull the quoted tokens out of those.
fn inline_string_values(description: Option<&str>) -> Vec<String> {
    let Some(desc) = description else {
        return Vec::new();
    };
    if !desc.contains("Range: string") {
        return Vec::new();
    }
    let mut values = Vec::new();
    let mut rest = desc;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        let token = &after[..close];
        if is_enum_token(token) {
            values.push(token.to_string());
        }
        rest = &after[close + 1..];
    }
    values.dedup();
    if values.len() >= 2 { values } else { Vec::new() }
}

#[derive(Debug, Clone)]
pub struct Data {
    pub option: Option<String>,
    pub description: Option<String>,
}

fn cell_text(el: &scraper::ElementRef) -> String {
    el.text().collect::<String>().trim().to_string()
}

fn opt(v: Option<&String>) -> Option<String> {
    v.filter(|s| !s.is_empty()).cloned()
}

pub fn parse_solver_options(html: &str) -> Vec<Data> {
    let document = Html::parse_document(html);

    let table_sel = Selector::parse(".markdownTable").unwrap();
    let th_sel = Selector::parse("th").unwrap();
    let td_sel = Selector::parse("td").unwrap();
    let tr_sel = Selector::parse("tr").unwrap();

    let mut results = Vec::new();

    for table in document.select(&table_sel) {
        let headers: Vec<String> = table
            .select(&th_sel)
            .map(|th| cell_text(&th))
            .filter(|s| !s.is_empty())
            .collect();

        let header_lower: Vec<String> = headers.iter().map(|h| h.to_lowercase()).collect();

        let has_option = header_lower.iter().any(|h| h == "option");
        let has_description = header_lower.iter().any(|h| h == "description");
        let has_default = header_lower.iter().any(|h| h == "default");

        if !(has_option && has_description && has_default) {
            continue;
        }

        let opt_idx = header_lower.iter().position(|h| h == "option").unwrap();
        let desc_idx = header_lower.iter().position(|h| h == "description").unwrap();
        let def_idx = header_lower.iter().position(|h| h == "default").unwrap();

        for row in table.select(&tr_sel) {
            if row.select(&th_sel).count() > 0 {
                continue;
            }

            let cells: Vec<String> = row.select(&td_sel).map(|td| cell_text(&td)).collect();

            let max_idx = opt_idx.max(desc_idx.max(def_idx));
            if cells.len() <= max_idx {
                continue;
            }

            results.push(Data {
                option: opt(cells.get(opt_idx)),
                description: opt(cells.get(desc_idx)),
            });
        }
    }

    results
}

fn to_snake_case(name: &str) -> String {
    // A name that already marks word boundaries explicitly (`Subsolver.Cplex.
    // MIPEmphasis`, `Flg_SLPMode`) is written in CamelCase within its segments.
    // A flat name (`CUTnrcuts`) uses the GAMS tag convention instead. This
    // decides the ambiguous case below.
    let structured = name.chars().any(|c| !c.is_alphanumeric());

    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();

    let mut result = String::with_capacity(sanitized.len() + 4);
    let chars: Vec<char> = sanitized.chars().collect();

    for i in 0..chars.len() {
        let c = chars[i];
        if c == '_' {
            result.push('_');
        } else if c.is_uppercase() {
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

            // An uppercase run followed by a lowercase word is ambiguous.
			// GAMS uses both conventions:
            //   `CUTnrcuts` = tag `CUT` + option `nrcuts` -> cut_nrcuts
            //   `ISolTol`   = CamelCase words             -> i_sol_tol
			//
            // Two signals mark the CamelCase case, in which the boundary belongs
            // before this letter rather than after the run: a further uppercase
            // later in the name (`ISolTol`), or a name that already separates its
            // words (`Subsolver.Cplex.MIPEmphasis` -> subsolver_cplex_mip_emphasis).
            let camel = prev_upper
                && next_lower
                && (structured || chars[i + 1..].iter().any(|c| c.is_uppercase()));

            if i > 0 {
                let prev = chars[i - 1];
                if prev.is_lowercase() || prev == '_' || camel {
                    result.push('_');
                }
            }
            result.push(c.to_ascii_lowercase());
            if prev_upper && next_lower && !camel {
                result.push('_');
            }
        } else {
            result.push(c);
        }
    }

    let collapsed: String = result
        .chars()
        .fold(String::with_capacity(result.len()), |mut acc, c| {
            if c == '_' && acc.ends_with('_') {
                return acc;
            }
            acc.push(c);
            acc
        });

    collapsed.trim_matches('_').to_string()
}

fn escape_keyword(name: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn",
        "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
        "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
        "self", "Self", "static", "struct", "super", "trait", "true", "type",
        "union", "unsafe", "use", "where", "while",
        "abstract", "become", "box", "do", "final", "macro", "override",
        "priv", "try", "typeof", "unsized", "virtual", "yield",
    ];
    if KEYWORDS.contains(&name) {
        format!("{}_", name)
    } else {
        name.to_string()
    }
}

/// Generates the bare `(kind, method, "key")` tuple lines for a single
/// solver's options, ready to feed a `gams_params!(...)` macro invocation.
///
/// - `kind` is `int`/`dbl`/`str`.
/// - `method` is the snake_case setter name.
/// - `"key"` is the exact GAMS option name written verbatim to the `.opt` file
///   (e.g. `limits/gap`, `MSK_IPAR_NUM_THREADS`, `AbsConFeasTol`). It is kept
///   raw.
///
/// Options with no name are skipped.
pub fn generate_solver_params(options: &[Data], details: &HashMap<String, Detail>) -> String {
    let mut out = String::new();

    for data in options {
        let Some(raw_name) = data.option.as_deref() else {
            continue;
        };

        let snake = to_snake_case(raw_name);
        let method = escape_keyword(&snake);
        let key = raw_name.replace('\\', "\\\\").replace('"', "\\\"");
        let detail = details.get(raw_name);

        let kind = match detail.and_then(|d| d.declared_type.as_deref()) {
            Some("integer") => "int",
            Some("real") => "dbl",
            Some("boolean") => "bool",
            Some("string") => "str",
            _ => "any",
        };

        let values = match detail.map(|d| d.string_values.clone()).unwrap_or_default() {
            v if !v.is_empty() => v,
            _ => inline_string_values(data.description.as_deref()),
        };

        if values.is_empty() {
            out.push_str(&format!("({kind}, {method}, \"{key}\"),\n"));
        } else {
            let list = values
                .iter()
                .map(|v| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("({kind}, {method}, \"{key}\", [{list}]),\n"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETAIL_HTML: &str = r#"<p><a class="anchor" id="CPLEXmipemphasis"></a><b>mipemphasis</b> <em>(integer)</em>: MIP solution tactics</p><blockquote class="doxtable">
<p>Default: <code>0</code></p>
<table class="markdownTable">
<tr class="markdownTableHead"><th>value   </th><th>meaning    </th></tr>
<tr><td><code>0</code></td><td>Balance optimality and feasibility</td></tr>
<tr><td><code>1</code></td><td>Emphasize feasibility</td></tr>
</table></blockquote>
<p><a class="anchor" id="MSKoptimizer"></a><b>MSK_IPAR_OPTIMIZER</b> <em>(string)</em>: optimizer</p><blockquote class="doxtable">
<table class="markdownTable">
<tr class="markdownTableHead"><th>value   </th><th>meaning    </th></tr>
<tr><td><code>MSK_OPTIMIZER_FREE</code></td><td>auto</td></tr>
<tr><td><code>MSK_OPTIMIZER_CONIC</code></td><td>conic</td></tr>
</table></blockquote>"#;

    #[test]
    fn test_detail_declared_type_is_captured() {
        let d = parse_option_details(DETAIL_HTML);
        assert_eq!(d["mipemphasis"].declared_type.as_deref(), Some("integer"));
        assert_eq!(d["MSK_IPAR_OPTIMIZER"].declared_type.as_deref(), Some("string"));
    }

    #[test]
    fn test_numeric_value_tables_are_not_enums() {
        // 0/1 codes would make useless variant names, so they are skipped.
        let d = parse_option_details(DETAIL_HTML);
        assert!(d["mipemphasis"].string_values.is_empty());
    }

    #[test]
    fn test_string_value_tables_become_enums() {
        let d = parse_option_details(DETAIL_HTML);
        assert_eq!(
            d["MSK_IPAR_OPTIMIZER"].string_values,
            vec!["MSK_OPTIMIZER_FREE", "MSK_OPTIMIZER_CONIC"]
        );
    }

    #[test]
    fn test_inline_string_values_from_description() {
        let desc = Some("LP solver: \"choose\", \"simplex\", \"ipm\"Range: string");
        assert_eq!(inline_string_values(desc), vec!["choose", "simplex", "ipm"]);
        assert!(inline_string_values(Some("uses \"a\", \"b\"")).is_empty());
    }

    #[test]
    fn test_snake_basic_pascal() {
        assert_eq!(to_snake_case("CUTnrcuts"), "cut_nrcuts");
        assert_eq!(to_snake_case("ECPmaster"), "ecp_master");
        assert_eq!(to_snake_case("MIPsolver"), "mip_solver");
        assert_eq!(to_snake_case("TOLepsf"), "tol_epsf");
        assert_eq!(to_snake_case("NLPcall"), "nlp_call");
    }

    #[test]
    fn test_snake_camel_after_acronym() {
        assert_eq!(to_snake_case("ISolTol"), "i_sol_tol");
        assert_eq!(to_snake_case("NLPIterLimit"), "nlp_iter_limit");
        assert_eq!(to_snake_case("FAPHeurLevel"), "fap_heur_level");
        assert_eq!(to_snake_case("QMatrixTol"), "q_matrix_tol");
        assert_eq!(to_snake_case("MipNLPIterLimit"), "mip_nlp_iter_limit");
    }

    #[test]
    fn test_snake_camel_in_structured_names() {
        assert_eq!(to_snake_case("Subsolver.Cplex.MIPEmphasis"), "subsolver_cplex_mip_emphasis");
        assert_eq!(to_snake_case("Subsolver.Gurobi.MIPFocus"), "subsolver_gurobi_mip_focus");
        assert_eq!(to_snake_case("Flg_SLPMode"), "flg_slp_mode");
        assert_eq!(to_snake_case("use_original_HFactor_logic"), "use_original_h_factor_logic");
    }

    #[test]
    fn test_snake_tag_form_is_preserved() {
        assert_eq!(to_snake_case("ExtNLPsolver"), "ext_nlp_solver");
        assert_eq!(to_snake_case("MIPoptcr"), "mip_optcr");
    }

    #[test]
    fn test_snake_all_lowercase() {
        assert_eq!(to_snake_case("reslim"), "reslim");
        assert_eq!(to_snake_case("solvelink"), "solvelink");
        assert_eq!(to_snake_case("solvetrace"), "solvetrace");
    }

    #[test]
    fn test_snake_trailing_acronym() {
        assert_eq!(to_snake_case("MIPoptcr"), "mip_optcr");
        assert_eq!(to_snake_case("TOLoptcr"), "tol_optcr");
        assert_eq!(to_snake_case("MIPoptimaliter"), "mip_optimaliter");
    }

    #[test]
    fn test_snake_with_dots() {
        assert_eq!(to_snake_case("output.debug.path"), "output_debug_path");
        assert_eq!(to_snake_case("subsolver.cplex.work_directory"), "subsolver_cplex_work_directory");
        assert_eq!(to_snake_case("primal.fixed_integer.call_strategy"), "primal_fixed_integer_call_strategy");
    }

    #[test]
    fn test_snake_with_leading_dot() {
        assert_eq!(to_snake_case(".equ_class"), "equ_class");
        assert_eq!(to_snake_case(".feaspref"), "feaspref");
        assert_eq!(to_snake_case(".lazy"), "lazy");
        assert_eq!(to_snake_case(".partition"), "partition");
    }

    #[test]
    fn test_snake_with_spaces() {
        assert_eq!(to_snake_case("central difference interval"), "central_difference_interval");
        assert_eq!(to_snake_case("feasibility tolerance"), "feasibility_tolerance");
        assert_eq!(to_snake_case("crash option"), "crash_option");
    }

    #[test]
    fn test_snake_mixed_case_with_dots() {
        assert_eq!(to_snake_case("dual.mip.solver"), "dual_mip_solver");
        assert_eq!(to_snake_case("dual.esh.interior_point.cutting_plane.time_limit"), "dual_esh_interior_point_cutting_plane_time_limit");
    }

    #[test]
    fn test_snake_consecutive_special_chars() {
        assert_eq!(to_snake_case("a..b"), "a_b");
        assert_eq!(to_snake_case("a  b"), "a_b");
        assert_eq!(to_snake_case("a._.b"), "a_b");
    }

    #[test]
    fn test_snake_already_snake_case() {
        assert_eq!(to_snake_case("cut_generation_epsilon"), "cut_generation_epsilon");
        assert_eq!(to_snake_case("max_number_nodes"), "max_number_nodes");
    }

    #[test]
    fn test_snake_leading_trailing_special() {
        assert_eq!(to_snake_case(".leading"), "leading");
        assert_eq!(to_snake_case("trailing."), "trailing");
        assert_eq!(to_snake_case(".both."), "both");
    }

    const BASIC_HTML: &str = r#"<table class="markdownTable">
<tr class="markdownTableHead">
<th>Option</th><th>Description</th><th>Default</th>
</tr>
<tr class="markdownTableRowOdd">
<td>CUTnrcuts</td><td>Cut generation pace</td><td><code>0</code></td>
</tr>
<tr class="markdownTableRowEven">
<td>ECPbeta</td><td>Updating multiplier</td><td><code>1.3</code></td>
</tr>
</table>"#;

    const SKIP_HTML: &str = r#"<table class="markdownTable">
<tr class="markdownTableHead">
<th>value</th><th>meaning</th>
</tr>
<tr class="markdownTableRowOdd">
<td>0</td><td>Off</td>
</tr>
</table>"#;

    #[test]
    fn test_parse_basic() {
        let data = parse_solver_options(BASIC_HTML);
        assert_eq!(data.len(), 2);

        assert_eq!(data[0].option.as_deref(), Some("CUTnrcuts"));
        assert_eq!(data[0].description.as_deref(), Some("Cut generation pace"));

        assert_eq!(data[1].option.as_deref(), Some("ECPbeta"));
    }

    #[test]
    fn test_parse_skips_bad_table() {
        let data = parse_solver_options(SKIP_HTML);
        assert!(data.is_empty());
    }

    #[test]
    fn test_generate_params_basic() {
        let data = parse_solver_options(BASIC_HTML);
        let generated = generate_solver_params(&data, &HashMap::new());

        assert!(generated.contains("(any, cut_nrcuts, \"CUTnrcuts\"),"));
        assert!(generated.contains("(any, ecp_beta, \"ECPbeta\"),"));
    }

    #[test]
    fn test_generate_params_keyword_escaped() {
        let html = r#"<table class="markdownTable">
<tr class="markdownTableHead">
<th>Option</th><th>Description</th><th>Default</th>
</tr>
<tr class="markdownTableRowOdd">
<td>continue</td><td>Continue option</td><td><code>0</code></td>
</tr>
</table>"#;
        let data = parse_solver_options(html);
        let generated = generate_solver_params(&data, &HashMap::new());

        assert!(generated.contains("(any, continue_, \"continue\"),"));
    }

    #[test]
    fn test_generate_params_string_kind() {
        let html = r#"<table class="markdownTable">
<tr class="markdownTableHead">
<th>Option</th><th>Description</th><th>Default</th>
</tr>
<tr class="markdownTableRowOdd">
<td>log file</td><td>Log file path</td><td><code>solve.log</code></td>
</tr>
</table>"#;
        let data = parse_solver_options(html);
        let generated = generate_solver_params(&data, &HashMap::new());

        assert!(generated.contains("(any, log_file, \"log file\"),"));
    }
}