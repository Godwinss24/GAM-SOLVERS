use scraper::{Html, Selector};

#[derive(Debug, Clone,)]
pub enum DataType {
    Integer,
    Float,
    String,
}

#[derive(Debug, Clone)]
pub struct Data {
    pub option: Option<String>,
    pub description: Option<String>,
    pub default: Option<String>,
    pub data_type: Option<DataType>,
}

fn cell_text(el: &scraper::ElementRef) -> String {
    el.text().collect::<String>().trim().to_string()
}

fn infer_type(v: &Option<String>) -> Option<DataType> {
    let val = v.as_ref()?;
    if val.parse::<i64>().is_ok() {
        Some(DataType::Integer)
    } else if val.parse::<f64>().is_ok() {
        Some(DataType::Float)
    } else {
        Some(DataType::String)
    }
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

            let default = opt(cells.get(def_idx));

            results.push(Data {
                option: opt(cells.get(opt_idx)),
                description: opt(cells.get(desc_idx)),
                data_type: infer_type(&default),
                default,
            });
        }
    }

    results
}

fn to_snake_case(name: &str) -> String {
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
            // A further uppercase later in the name marks the CamelCase case, so
            // the word boundary belongs before this letter rather than after
            // the run.
            let camel =
                prev_upper && next_lower && chars[i + 1..].iter().any(|c| c.is_uppercase());

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

/// Maps an inferred [`DataType`] to the `kind` tag used by the
/// `gurobi_params!`-style macro (`int`, `dbl`, `str`).
fn type_kind_str(dt: &Option<DataType>) -> &'static str {
    match dt {
        Some(DataType::Integer) => "int",
        Some(DataType::Float) => "dbl",
        Some(DataType::String) | None => "str",
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
pub fn generate_solver_params(options: &[Data]) -> String {
    let mut out = String::new();

    for data in options {
        let Some(raw_name) = data.option.as_deref() else {
            continue;
        };

        let snake = to_snake_case(raw_name);
        let method = escape_keyword(&snake);
        let kind = type_kind_str(&data.data_type);
        let key = raw_name.replace('\\', "\\\\").replace('"', "\\\"");

        out.push_str(&format!("({kind}, {method}, \"{key}\"),\n"));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_infer_integer() {
        assert!(matches!(infer_type(&Some("0".into())), Some(DataType::Integer)));
        assert!(matches!(infer_type(&Some("50".into())), Some(DataType::Integer)));
        assert!(matches!(infer_type(&Some("-1".into())), Some(DataType::Integer)));
        assert!(matches!(infer_type(&Some("200".into())), Some(DataType::Integer)));
    }

    #[test]
    fn test_infer_float() {
        assert!(matches!(infer_type(&Some("1.3".into())), Some(DataType::Float)));
        assert!(matches!(infer_type(&Some("2.0".into())), Some(DataType::Float)));
        assert!(matches!(infer_type(&Some("1e-3".into())), Some(DataType::Float)));
        assert!(matches!(infer_type(&Some("1e10".into())), Some(DataType::Float)));
        assert!(matches!(infer_type(&Some("1e-6".into())), Some(DataType::Float)));
    }

    #[test]
    fn test_infer_string() {
        assert!(matches!(infer_type(&Some("GAMS MIP solver".into())), Some(DataType::String)));
        assert!(matches!(infer_type(&Some("GAMS optCR".into())), Some(DataType::String)));
        assert!(matches!(infer_type(&Some("Filename".into())), Some(DataType::String)));
    }

    #[test]
    fn test_infer_none() {
        assert!(infer_type(&None).is_none());
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
        assert_eq!(data[0].default.as_deref(), Some("0"));
        assert!(matches!(data[0].data_type, Some(DataType::Integer)));

        assert_eq!(data[1].option.as_deref(), Some("ECPbeta"));
        assert_eq!(data[1].default.as_deref(), Some("1.3"));
        assert!(matches!(data[1].data_type, Some(DataType::Float)));
    }

    #[test]
    fn test_parse_skips_bad_table() {
        let data = parse_solver_options(SKIP_HTML);
        assert!(data.is_empty());
    }

    #[test]
    fn test_generate_params_basic() {
        let data = parse_solver_options(BASIC_HTML);
        let generated = generate_solver_params(&data);

        assert!(generated.contains("(int, cut_nrcuts, \"CUTnrcuts\"),"));
        assert!(generated.contains("(dbl, ecp_beta, \"ECPbeta\"),"));
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
        let generated = generate_solver_params(&data);

        assert!(generated.contains("(int, continue_, \"continue\"),"));
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
        let generated = generate_solver_params(&data);

        assert!(generated.contains("(str, log_file, \"log file\"),"));
    }
}