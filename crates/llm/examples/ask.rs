//! One turn through the claude CLI, without applying it:
//! cargo run -p llm --example ask -- <patches root> <patch folder or -> "<message>" [model]
use llm::{Cancel, ClaudeCli, Turn, find_claude, run};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let turn = Turn {
        root: a[0].clone().into(),
        current: (a[1] != "-").then(|| (a[1].clone(), "(defaults)".into())),
        history: vec![],
        text: a[2].clone(),
        capture: None,
    };
    let backend = ClaudeCli { bin: find_claude().expect("claude not found"), model: a.get(3).cloned().unwrap_or_default(), config_dir: None };
    let t = std::time::Instant::now();
    match run(&turn, &backend, &Cancel::default(), &|p| eprintln!("{:5.1} s {p:?}", t.elapsed().as_secs_f32())) {
        Ok(o) => {
            println!("{}\n", o.reply);
            for (path, text) in o.proposal.iter().flat_map(|p| &p.files) {
                println!("--- {path}\n{}", text.as_deref().unwrap_or("(deleted)"));
            }
        }
        Err(e) => println!("{e:?}"),
    }
}
