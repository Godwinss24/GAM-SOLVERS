mod constants;
mod fetch;
mod parse;
mod utils;

use constants::*;
use fetch::*;
use parse::generate_solver_params;

supported_solvers! {
    ALPHAECP => "ALPHAECP";
    ANTIGONE => "ANTIGONE";
    BARON => "BARON";
    CBC => "CBC";
    CONOPT => "CONOPT";
    CONOPT3 => "CONOPT3";
    COPT => "COPT";
    CPLEX => "CPLEX";
    DECIS => "DECIS";
    DICOPT => "DICOPT";
    GUROBI => "GUROBI";
    HIGHS => "HIGHS";
    IPOPT => "IPOPT";
    KNITRO => "KNITRO";
    LINDO => "LINDO";
    MINOS => "MINOS";
    MOSEK => "MOSEK";
    ODHCPLEX => "ODHCPLEX";
    SBB => "SBB";
    SCIP => "SCIP";
    SHOT => "SHOT";
    SNOPT => "SNOPT";
    SOPLEX => "SOPLEX";
    XPRESS => "XPRESS";
}

#[tokio::main]
async fn main() {
    // No arg means that you sscrape every solver i have hardcodedd
    // e.g. `cargo run -- GUROBI`.
    let filter = std::env::args().nth(1);

    for &solver in SupportedSolver::ALL {
        if let Some(ref f) = filter {
            if !solver.url_name().eq_ignore_ascii_case(f) {
                continue;
            }
        }

        let link = format!("{BASE_URL}S_{}.html", solver.url_name());
        eprintln!("Fetching: {link}");

        match scrape_gams_solvers(&link).await {
            Ok((data, details)) => {
                let typed = details.values().filter(|d| d.declared_type.is_some()).count();
                let enums = details.values().filter(|d| !d.string_values.is_empty()).count();
                eprintln!(
                    "  -> {} options ({typed} with a declared type, {enums} enumerated)",
                    data.len()
                );
                let params = generate_solver_params(&data, &details);
                println!("// {}", solver.url_name());
                print!("{params}");
            }
            Err(e) => {
                eprintln!("  -> Error: {e}");
            }
        }
    }

    if let Some(f) = filter {
        if !SupportedSolver::ALL
            .iter()
            .any(|s| s.url_name().eq_ignore_ascii_case(&f))
        {
            eprintln!("Unknown solver: {f}");
            eprintln!(
                "Supported: {}",
                SupportedSolver::ALL
                    .iter()
                    .map(|s| s.url_name())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            std::process::exit(1);
        }
    }
}
