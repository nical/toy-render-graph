extern crate rendergraph;
#[macro_use]
extern crate serde;

use rendergraph::*;
use guillotiere::euclid::size2;
use guillotiere::AllocatorOptions;
use clap::*;

use std::io::prelude::*;
use std::fs::{File, OpenOptions};
use std::collections::HashMap;

fn main() {
    let matches = App::new("Render graph command-line interface")
        .version("0.1")
        .author("Nicolas Silva <nical@fastmail.com>")
        .about("Render-task scheduling.")
        .subcommand(
            SubCommand::with_name("init")
            .about("Initialize the graph")
            .arg(Arg::with_name("WIDTH")
                .help("Default texture width.")
                .value_name("WIDTH")
                .takes_value(true)
                .required(true)
            )
            .arg(Arg::with_name("HEIGHT")
                .help("Default texture height.")
                .value_name("HEIGHT")
                .takes_value(true)
                .required(true)
            )
            .arg(Arg::with_name("LARGE_SIZE")
                .short("l")
                .long("large")
                .help("Size above which a rectangle is considered large")
                .value_name("LARGE")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("SMALL_SIZE")
                .short("s")
                .long("small")
                .help("Size above which a rectangle is considered large")
                .value_name("LARGE")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("SNAP")
                .long("snap")
                .help("Round up the size of the allocated rectangle to a multiple of the provided value.")
                .value_name("SNAP")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("GRAPH")
                .short("g")
                .long("graph")
                .help("Sets the output graph file to use")
                .value_name("FILE")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("SVG_OUTPUT")
                .long("svg")
                .help("Dump the graph in an SVG file")
                .value_name("SVG_OUTPUT")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("BUILD")
                .long("build")
                .help("Build the graph")
                .value_name("BUILD")
                .takes_value(false)
                .required(false)
            )
        )
        .subcommand(
            SubCommand::with_name("node")
            .about("Add a node")
            .arg(Arg::with_name("WIDTH")
                .help("Rectangle width.")
                .value_name("WIDTH")
                .takes_value(true)
                .required(true)
            )
            .arg(Arg::with_name("HEIGHT")
                .help("Rectangle height.")
                .value_name("HEIGHT")
                .takes_value(true)
                .required(true)
            )
            .arg(Arg::with_name("NAME")
                .short("-n")
                .long("name")
                .help("Set a name to identify the rectangle.")
                .value_name("NAME")
                .takes_value(true)
                .required(false)
             )
            .arg(Arg::with_name("GRAPH")
                .short("g")
                .long("graph")
                .help("Sets the output graph file to use")
                .value_name("FILE")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("INPUT")
                .short("i")
                .long("input")
                .help("Input dependency of the node")
                .value_name("FILE")
                .takes_value(true)
                .multiple(true) // values_of
                .required(false)
            )
            .arg(Arg::with_name("TARGET_KIND")
                .short("t")
                .long("target")
                .help("Render target kind.")
                .value_name("TARGET_KIND")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("FIXED_ALLOC")
                .short("f")
                .long("fixed")
                .help("Whether the target allocation is dynamic or fixed.")
                .value_name("FIXED_ALLOC")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("ROOT")
                .short("r")
                .long("root")
                .help("Whether the node is a root.")
                .value_name("ROOT")
                .takes_value(false)
                .required(false)
            )
            .arg(Arg::with_name("SVG_OUTPUT")
                .long("svg")
                .help("Dump the graph in an SVG file")
                .value_name("SVG_OUTPUT")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("BUILD")
                .long("build")
                .help("Build the graph")
                .value_name("BUILD")
                .takes_value(false)
                .required(false)
            )
        )
        .subcommand(
            SubCommand::with_name("root")
            .about("Add a node")
            .arg(Arg::with_name("NAME")
                .help("Mark a node as root.")
                .value_name("NAME")
                .takes_value(true)
                .required(true)
            )
            .arg(Arg::with_name("GRAPH")
                .short("g")
                .long("graph")
                .help("Sets the output graph file to use")
                .value_name("FILE")
                .takes_value(true)
                .required(false)
            )
            .arg(Arg::with_name("SVG_OUTPUT")
                .long("svg")
                .help("Dump the graph in an SVG file")
                .value_name("SVG_OUTPUT")
                .takes_value(true)
                .required(false)
            )
        )
        .subcommand(
            SubCommand::with_name("svg")
            .about("Dump the graph as SVG")
            .arg(Arg::with_name("GRAPH")
                .short("-a")
                .long("graph")
                .help("Input graph file.")
                .value_name("GRAPH")
                .takes_value(true)
             )
            .arg(Arg::with_name("SVG_OUTPUT")
                .help("Output SVG file to use")
                .value_name("FILE")
                .takes_value(true)
                .required(false)
            )
        )
        .subcommand(
            SubCommand::with_name("list")
            .about("List the nodes in the graph")
            .arg(Arg::with_name("GRAPH")
                .short("-a")
                .long("graph")
                .help("Input graph file.")
                .value_name("GRAPH")
                .takes_value(true)
             )
        )
        .get_matches();

    if let Some(cmd) = matches.subcommand_matches("init") {
        init(cmd);
    } else if let Some(cmd) = matches.subcommand_matches("node") {
        node(cmd);
    } else if let Some(cmd) = matches.subcommand_matches("root") {
        root(cmd);
    } else if let Some(cmd) = matches.subcommand_matches("build") {
        let mut session = load_graph(cmd);
        build(&mut session);
        write_graph(&session, cmd);
    } else if let Some(cmd) = matches.subcommand_matches("svg") {
        svg(cmd);
    } else if let Some(cmd) = matches.subcommand_matches("list") {
        list(cmd);
    }
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    graph: Graph,
    built_graph: Option<BuiltGraph>,
    names: HashMap<String, NodeId>,
    allocator_options: AllocatorOptions,
    default_size: Size,
    next_id: i32,
}

fn init(args: &ArgMatches) {
    let w = args.value_of("WIDTH").map(|s| s.parse::<i32>().unwrap()).unwrap_or(1024);
    let h = args.value_of("HEIGHT").map(|s| s.parse::<i32>().unwrap()).unwrap_or(1024);

    let default_options = guillotiere::DEFAULT_OPTIONS;

    let allocator_options = AllocatorOptions {
        snap_size: args.value_of("SNAP")
            .map(|s| s.parse::<i32>().unwrap())
            .unwrap_or(default_options.snap_size),
        small_size_threshold: args.value_of("SMALL")
            .map(|s| s.parse::<i32>().unwrap())
            .unwrap_or(default_options.small_size_threshold),
        large_size_threshold: args.value_of("LARGE")
            .map(|s| s.parse::<i32>().unwrap())
            .unwrap_or(default_options.large_size_threshold),
    };

    let session = Session {
        graph: Graph::new(),
        built_graph: None,
        names: std::collections::HashMap::default(),
        allocator_options,
        default_size: size2(w, h),
        next_id: 0,
    };

    write_graph(&session, &args);

    if args.is_present("SVG_OUTPUT") {
        svg(args);
    }
}

fn build(session: &mut Session) {
    let mut builder = GraphBuilder::new(BuilderOptions {
        passes: PassOptions::Recursive,
        targets: TargetOptions::PingPong,
        culling: true,
    });
    let mut allocator = GuillotineAllocator::with_options(session.default_size, &session.allocator_options);
    session.built_graph = Some(builder.build(session.graph.clone(), &mut allocator));
}

fn node(args: &ArgMatches) {
    let mut session = load_graph(args);

    let mut inputs = Vec::new();
    if let Some(names) = args.values_of("INPUT") {
        for name in names {
            inputs.push(session.names[name]);
        }
    }

    let name = args.value_of("NAME").map(|name| name.to_string()).unwrap_or_else(|| {
        session.next_id += 1;
        format!("#{}", session.next_id)
    });

    let target_kind = match args.value_of("TARGET_KIND") {
        Some("Alpha") | Some("alpha") => TargetKind::Alpha,
        _ => TargetKind::Color,
    };

    let alloc_kind = match args.value_of("FIXED_ALLOC") {
        Some(_) => AllocKind::Fixed(TextureId(1337)),
        None => AllocKind::Dynamic,
    };

    let w = args.value_of("WIDTH").expect("Missing width.").parse::<i32>().unwrap();
    let h = args.value_of("HEIGHT").expect("Missing height.").parse::<i32>().unwrap();

    let id = session.graph.add_node(&name, target_kind, size2(w, h), alloc_kind, &inputs[..]);

    if args.is_present("ROOT") {
        session.graph.add_root(id);
    }

    session.names.insert(name, id);

    build(&mut session);

    write_graph(&session, args);

    if args.is_present("SVG_OUTPUT") {
        svg(args);
    }
}

fn root(args: &ArgMatches) {
    let mut session = load_graph(args);

    let name = args.value_of("NAME").unwrap().to_string();
    let id = session.names.get(&name).expect("Couldn't find node with this name.");

    session.graph.add_root(*id);

    build(&mut session);

    write_graph(&session, args);

    if args.is_present("SVG_OUTPUT") {
        svg(args);
    }
}

fn svg(args: &ArgMatches) {
    let session = load_graph(args);

    let svg_file_name = args.value_of("SVG_OUTPUT").unwrap_or("rendergraph.svg");
    let mut svg_file = File::create(svg_file_name).expect(
        "Failed to open the SVG file."
    );

    let mut builder = GraphBuilder::new(BuilderOptions {
        passes: PassOptions::Recursive,
        targets: TargetOptions::PingPong,
        culling: true,
    });
    let mut allocator = GuillotineAllocator::with_options(session.default_size, &session.allocator_options);
    let built_graph = builder.build(session.graph.clone(), &mut allocator);
    rendergraph::dump_svg(&mut svg_file, &built_graph, &allocator);
}

fn list(args: &ArgMatches) {
    let session = load_graph(args);

    println!("# Nodes");
    for (name, _) in &session.names {
        println!(" - {}", name);
    }
    println!("# Roots");
    for &root in session.graph.roots() {
        for (name, id) in &session.names {
            if *id == root {
                println!(" - {}", name);
                continue;
            }
        }
    }
}

fn load_graph(args: &ArgMatches) -> Session {
    let file_name = args.value_of("GRAPH").unwrap_or("rendergraph.ron");
    let file = OpenOptions::new().read(true).open(file_name).expect(
        "Failed to open the graph file."
    );

    ron::de::from_reader(file).expect("Failed to parse the graph")
}

fn write_graph(session: &Session, args: &ArgMatches) {
    let serialized: String = ron::ser::to_string_pretty(
        &session,
        ron::ser::PrettyConfig::default(),
    ).unwrap();

    let file_name = args.value_of("GRAPH").unwrap_or("rendergraph.ron");
    let mut graph_file = std::fs::File::create(file_name).expect(
        "Failed to open the graph file."
    );

    graph_file.write_all(serialized.as_bytes()).expect(
        "Failed to write into the graph file."
    );
}

