mod constants;
mod fetch;
mod parse;
mod utils;

use constants::*;
use fetch::*;
use parse::generate_solver_params;

supported_solvers! {
    BARON => "BARON";
    GUROBI => "GUROBI";
    HIGHS => "HIGHS";
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
            Ok(data) => {
                eprintln!("  -> {} options", data.len());
                let params = generate_solver_params(&data);
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
