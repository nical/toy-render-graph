
pub use std::i32;
use crate::allocator::*;
pub use crate::allocator::{TextureId, TextureAllocator, GuillotineAllocator, DbgTextureAllocator};

pub use guillotiere::{Rectangle, Size, Point};
pub use euclid::{size2, vec2, point2};

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub fn to_usize(self) -> usize { self.0 as usize }
}

fn node_id(idx: usize) -> NodeId {
    debug_assert!(idx < std::u32::MAX as usize);
    NodeId(idx as u32)
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Color = 0,
    Alpha = 1,
}

const NUM_TARGET_KINDS: usize = 2;

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Node {
    pub task_kind: TaskKind,
    pub size: Size,
    pub alloc_kind: AllocKind,
    pub dependencies: Vec<NodeId>,
    pub target_kind: TargetKind,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AllocKind {
    Fixed(TextureId),
    Dynamic,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Blit,
    Render(u32),
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Graph {
    nodes: Vec<Node>,
    roots: Vec<NodeId>,
}

impl Graph {
    pub fn new() -> Self {
        Graph {
            nodes: Vec::new(),
            roots: Vec::new(),
        }
    }

    pub fn add_node(&mut self, task_kind: TaskKind, target_kind: TargetKind, size: Size, alloc_kind: AllocKind, deps: &[NodeId]) -> NodeId {
        let id = node_id(self.nodes.len());
        self.nodes.push(Node {
            task_kind,
            size,
            alloc_kind,
            dependencies: deps.to_vec(),
            target_kind,
        });

        id
    }

    pub fn add_root(&mut self, id: NodeId) {
        self.roots.push(id);
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct BuiltGraph {
    pub nodes: Vec<Node>,
    pub roots: Vec<NodeId>,
    pub allocated_rects: Vec<Option<AllocatedRect>>,
    pub passes: Vec<Pass>,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct Pass {
    pub targets: [PassTarget; NUM_TARGET_KINDS],
    pub fixed_target: bool,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct PassTarget {
    pub(crate) nodes: Vec<NodeId>,
    pub(crate) destination: Option<TextureId>,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetDestination {
    Dynamic(TextureId),
    Fixed(TextureId),
}


#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PassOptions {
    Recursive,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetOptions {
    Direct,
    PingPong,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BuilderOptions {
    pub passes: PassOptions,
    pub targets: TargetOptions,
    pub culling: bool,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct GraphBuilder {
    options: BuilderOptions,
}

impl GraphBuilder {
    pub fn new(options: BuilderOptions) -> Self {
        GraphBuilder { options }
    }

    pub fn build(&mut self, mut graph: Graph, allocator: &mut dyn TextureAllocator) -> BuiltGraph {

        // Step 1 - Sort the nodes in a valid execution order (nodes appear after
        // the nodes they depend on.
        //
        // We don't have to do anything here because this property is provided
        // by construction in the Graph data structure.
        // The next step depends on this property.

        // Step 2 - Mark nodes that contribute to the result of the root nodes
        // as active.
        //
        // This pass is not strictly necessary, but, it prevents nodes that don't
        // contrbute to the result of the root nodes from being executed.

        let mut active_nodes = Vec::new();
        if self.options.culling {
            cull_nodes(&graph, &mut active_nodes);
        } else {
            active_nodes = vec![true; graph.nodes.len()];
        }

        let mut passes = Vec::new();
        let mut node_passes = vec![i32::MAX; graph.nodes.len()];


        // Step 3 - Assign nodes to passes.
        //
        // The main constraint is that nodes must be in a later pass than
        // all of their dependencies.

        match self.options.passes {
            PassOptions::Recursive => create_passes_recursive(
                &graph,
                &mut active_nodes,
                &mut passes,
                &mut node_passes,
            ),
        }

        // Step 4 - assign render targets to passes.
        //
        // A render target can be used by several passes as long as no pass
        // both read and write the same render target.

        match self.options.targets {
            TargetOptions::Direct => assign_targets_direct(
                &mut passes,
                &mut graph.nodes,
                &mut node_passes,
                allocator,
            ),
            TargetOptions::PingPong => assign_targets_ping_pong(
                &mut passes,
                &mut graph.nodes,
                &mut node_passes,
                allocator,
            ),
        }

        // Step 5 - Allocate sub-rects of the render targets for each node.
        //
        // Several nodes can alias parts of a render target as long no node
        // overwite the result of a node that will be needed later.

        let mut allocated_rects = vec![None; graph.nodes.len()];
        reset(&mut active_nodes, graph.nodes.len(), false);
        allocate_target_rects(
            &graph,
            &passes,
            &mut active_nodes,
            &mut allocated_rects,
            allocator,
        );

        BuiltGraph {
            nodes: graph.nodes,
            roots: graph.roots,
            allocated_rects,
            passes,
        }
    }
}

fn reset<T: Copy>(vector: &mut Vec<T>, len: usize, val: T) {
    vector.clear();
    vector.reserve(len);
    for _ in 0..len {
        vector.push(val);
    }
}

/// Generate a vector containing for each node a boolean which is true if the node contributes to
/// a root of the graph.
fn cull_nodes(graph: &Graph, active_nodes: &mut Vec<bool>) {
    // Call this function recursively starting from the roots.
    fn mark_active_node(id: NodeId, nodes: &[Node], active_nodes: &mut [bool]) {
        let idx = id.to_usize();
        if active_nodes[idx] {
            return;
        }

        active_nodes[idx] = true;

        for &dep in &nodes[idx].dependencies {
            mark_active_node(dep, nodes, active_nodes);
        }
    }

    reset(active_nodes, graph.nodes.len(), false);

    for &root in &graph.roots {
        mark_active_node(root, &graph.nodes, active_nodes);
    }
}

/// Create render passes and assign the nodes to them.
///
/// This method tries to emulate WebRender's current behavior.
/// Nodes are executed as late as possible.
///
/// TODO: Support fixed target allocations.
fn create_passes_recursive(
    graph: &Graph,
    active_nodes: &[bool],
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32],
) {
    fn create_passes_recursive_impl(
        node_id: NodeId,
        rev_pass_index: usize,
        nodes: &[Node],
        node_rev_passes: &mut [usize],
        max_depth: &mut usize,
    ) {
        *max_depth = std::cmp::max(*max_depth, rev_pass_index);

        node_rev_passes[node_id.to_usize()] = std::cmp::max(
            node_rev_passes[node_id.to_usize()],
            rev_pass_index,
        );

        let node = &nodes[node_id.to_usize()];
        for &dep in &node.dependencies {
            create_passes_recursive_impl(
                dep,
                rev_pass_index + 1,
                nodes,
                node_rev_passes,
                max_depth,
            );
        }
    }

    let mut max_depth = 0;
    let mut node_rev_passes = vec![0; graph.nodes.len()];

    for &root in &graph.roots {
        create_passes_recursive_impl(
            root,
            0,
            &graph.nodes,
            &mut node_rev_passes,
            &mut max_depth,
        );
    }

    for _ in 0..(max_depth + 1) {
        passes.push(Pass {
            targets: [
                PassTarget {
                    nodes: Vec::new(),
                    destination: None,
                },
                PassTarget {
                    nodes: Vec::new(),
                    destination: None,
                },
            ],
            fixed_target: false,
        });
    }

    for node_idx in 0..graph.nodes.len() {
        if !active_nodes[node_idx] {
            continue;
        }

        let target_kind = graph.nodes[node_idx].target_kind;
        let pass_index = max_depth - node_rev_passes[node_idx];
        passes[pass_index].targets[target_kind as usize].nodes.push(node_id(node_idx));
        node_passes[node_idx] = pass_index as i32;
    }
}

/// Assign a render target to each pass with a "ping-pong" scheme alternating between
/// two render targets.
///
/// In order to ensure that a node never reads and writes from the same target, some
/// blit nodes may be inserted in the graph.
fn assign_targets_ping_pong(
    passes: &mut[Pass],
    nodes: &mut Vec<Node>,
    node_passes: &mut [i32],
    allocator: &mut dyn TextureAllocator,
) {
    let mut node_redirects = vec![None; nodes.len()];

    let texture_ids = [
        // color
        [
            allocator.add_texture(),
            allocator.add_texture(),
        ],
        // alpha
        [
            allocator.add_texture(),
            allocator.add_texture(),
        ],
    ];

    let mut last_dynamic_pass = 0;
    let mut nth_dynamic_pass = [0, 0];
    for p in 0..passes.len() {
        if passes[p].fixed_target {
            continue;
        }

        for target_kind_index in 0..NUM_TARGET_KINDS {
            if passes[p].targets[target_kind_index].nodes.is_empty() {
                continue;
            }

            let ping_pong = nth_dynamic_pass[target_kind_index] % 2;
            nth_dynamic_pass[target_kind_index] += 1;

            let current_destination = texture_ids[target_kind_index][ping_pong];
            passes[p].targets[target_kind_index].destination = Some(current_destination);

            for nth_node in 0..passes[p].targets[target_kind_index].nodes.len() {
                let n = passes[p].targets[target_kind_index].nodes[nth_node].to_usize();
                for nth_dep in 0..nodes[n].dependencies.len() {
                    let dep = nodes[n].dependencies[nth_dep].to_usize();
                    let dep_pass = node_passes[dep] as usize;
                    let dep_target_kind = nodes[dep].target_kind;
                    // Can't both read and write the same target.
                    if passes[dep_pass].targets[dep_target_kind as usize].destination == Some(current_destination) {

                        // See if we have already added a blit task to avoid the problem.
                        let source = if let Some(source) = node_redirects[dep] {
                            source
                        } else {
                            // Otherwise add a blit task.
                            let blit_id = node_id(nodes.len());
                            let size = nodes[dep].size;
                            let target_kind = nodes[dep].target_kind;
                            nodes.push(Node {
                                task_kind: TaskKind::Blit,
                                dependencies: vec![node_id(dep)],
                                alloc_kind: AllocKind::Dynamic,
                                size,
                                target_kind,
                            });
                            node_redirects[dep] = Some(blit_id);

                            passes[last_dynamic_pass].targets[dep_target_kind as usize].nodes.push(blit_id);

                            blit_id
                        };

                        nodes[n].dependencies[nth_dep] = source;
                    }
                }
            }
        }
        if !passes[p].fixed_target {
            last_dynamic_pass = p;
        }
    }
}

/// Assign a render target to each pass without adding nodes to the graph.
///
/// This method may generate more render targets than assign_targets_ping_pong,
/// however it doesn't add any extra copying operations.
fn assign_targets_direct(
    passes: &mut[Pass],
    nodes: &mut Vec<Node>,
    node_passes: &mut [i32],
    allocator: &mut dyn TextureAllocator,
) {
    let mut allocated_textures = [Vec::new(), Vec::new()];
    let mut dependencies = std::collections::HashSet::new();

    for p in 0..passes.len() {
        let pass = &passes[p];
        if pass.fixed_target {
            continue;
        }

        dependencies.clear();
        for target in &pass.targets {
            for &pass_node in &target.nodes {
                for &dep in &nodes[pass_node.to_usize()].dependencies {
                    let dep_pass = node_passes[dep.to_usize()];
                    let target_kind = nodes[dep.to_usize()].target_kind;
                    if let Some(id) = passes[dep_pass as usize].targets[target_kind as usize].destination {
                        dependencies.insert(id);
                    }
                }
            }
        }

        for target_kind in 0..NUM_TARGET_KINDS {
            if passes[p].targets[target_kind].nodes.is_empty() {
                continue;
            }

            let mut destination = None;
            for target_id in &allocated_textures[target_kind] {
                if !dependencies.contains(target_id) {
                    destination = Some(*target_id);
                    break;
                }
            }

            let destination = destination.unwrap_or_else(|| {
                let id = allocator.add_texture();
                allocated_textures[target_kind].push(id);
                id
            });

            passes[p].targets[target_kind].destination = Some(destination);
        }
    }
}

/// Determine when is the first and last time that the sub-rect associated to the
/// result of each node is needed and allocate portions of the render targets
/// accordingly.
/// This method computes the lifetime of each node and delegates the allocation
/// logic to the TextureAllocator implementation.
fn allocate_target_rects(
    graph: &Graph,
    passes: &[Pass],
    visited: &mut[bool],
    allocated_rects: &mut Vec<Option<AllocatedRect>>,
    allocator: &mut dyn TextureAllocator,
) {
    let mut last_node_refs: Vec<NodeId> = Vec::with_capacity(graph.nodes.len());
    let mut pass_last_node_ranges: Vec<std::ops::Range<usize>> = vec![0..0; passes.len()];

    // The first step is to find for each pass the list of nodes that are not referenced
    // anymore after the pass ends.

    // Mark roots as visited to avoid deallocating their target rects.
    for root in &graph.roots {
        visited[root.to_usize()] = true;
    }

    // Visit passes in reverse order and look at the dependencies.
    // Each dependency that we haven't visited yet is the last reference to a node.
    let mut pass_index = passes.len();
    for pass in passes.iter().rev() {
        pass_index -= 1;
        let first = last_node_refs.len();
        for target_kind in 0..NUM_TARGET_KINDS {
            for &n in &pass.targets[target_kind].nodes {
                for &dep in &graph.nodes[n.to_usize()].dependencies {
                    let dep_idx = dep.to_usize();
                    if !visited[dep_idx] {
                        visited[dep_idx] = true;
                        last_node_refs.push(dep);
                    }
                }
            }
        }
        pass_last_node_ranges[pass_index] = first..last_node_refs.len();
    }

    // In the second step we go through each pass in order and perform allocations/deallocations.
    for (pass_index, pass) in passes.iter().enumerate() {
        if pass.fixed_target {
            continue;
        }

        for pass_target in &pass.targets {
            if pass_target.nodes.is_empty() {
                continue;
            }
            let texture = pass_target.destination.unwrap();

            // Allocations needed for this pass.
            for &node in &pass_target.nodes {
                let node_idx = node.to_usize();
                let size = graph.nodes[node_idx].size;
                allocated_rects[node_idx] = Some(allocator.allocate(texture, size));
            }
        }

        // Deallocations we can perform after this pass.
        let finished_range = pass_last_node_ranges[pass_index].clone();
        for finished_node in &last_node_refs[finished_range] {
            let node_idx = finished_node.to_usize();
            if let Some(allocated_rect) = allocated_rects[node_idx] {
                allocator.deallocate(
                    allocated_rect.id,
                );
            }
        }
    }
}

pub fn build_and_print_graph(graph: &Graph, options: BuilderOptions, with_deallocations: bool) {
    let mut builder = GraphBuilder::new(options);
    let mut allocator = GuillotineAllocator::new(size2(1024, 1024));
    let mut allocator = DbgTextureAllocator::new(&mut allocator);
    allocator.record_deallocations = with_deallocations;

    let built_graph = builder.build(graph.clone(), &mut allocator);

    let n_passes = built_graph.passes.len();
    let mut n_nodes = 0;
    for pass in &built_graph.passes {
        for target in &pass.targets {
            n_nodes += target.nodes.len();
        }
    }

    println!(
        "\n\n------------- deallocations: {:?}, culling: {:?}, passes: {:?}, targets: {:?}",
        with_deallocations,
        options.culling,
        options.passes,
        options.targets
    );
    println!(
        "              {:?} nodes, {:?} passes, {:?} targets, {:?} pixels (max), {:?} rects (max)",
        n_nodes,
        n_passes,
        allocator.textures.len(),
        allocator.max_allocated_pixels(),
        allocator.max_allocated_rects(),
    );

    for i in 0..built_graph.passes.len() {
        let pass = &built_graph.passes[i];
        println!("# pass {:?}", i);
        for &target_kind in &[TargetKind::Color, TargetKind::Alpha] {
            if let Some(texture) = pass.targets[target_kind as usize].destination {
                println!("  * {} {:?} target {:?}:",
                    if pass.fixed_target { "  fixed" } else { "dynamic" },
                    target_kind,
                    texture,
                );
                for &node in &pass.targets[target_kind as usize].nodes {
                    println!("     - {:?} {:?}      {}",
                        node,
                        built_graph.nodes[node.to_usize()].task_kind,
                        if let Some(r) = built_graph.allocated_rects[node.to_usize()] {
                            format!("rect: [({}, {}) {}x{}]", r.rectangle.min.x, r.rectangle.min.y, r.rectangle.size().width, r.rectangle.size().height)
                        } else {
                            "".to_string()
                        },
                    );
                }
            }
        }
    }
}


#[test]
fn simple_graph() {
    let mut graph = Graph::new();

    let n0 = graph.add_node("useless", TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[]);
    let _ = graph.add_node("useless", TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n0]);
    let n2 = graph.add_node("n2", TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[]);
    let n3 = graph.add_node("n3", TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[]);
    let n4 = graph.add_node("n4", TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[n2, n3]);
    let n5 = graph.add_node("n5", TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[]);
    let n6 = graph.add_node("n6", TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n3, n5]);
    let n7 = graph.add_node("n7", TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n2, n4, n6]);
    let n8 = graph.add_node("root", TargetKind::Color, size2(800, 600), AllocKind::Fixed(TextureId(100)), &[n7]);

    graph.add_root(n5);
    graph.add_root(n8);

    for &with_deallocations in &[true] {
        for &pass_option in &[PassOptions::Recursive] {
            for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
                build_and_print_graph(
                    &graph,
                    BuilderOptions {
                        passes: pass_option,
                        targets: target_option,
                        culling: true,
                    },
                    with_deallocations,
                )
            }
        }
    }
}

#[test]
fn test_stacked_shadows() {
    let mut graph = Graph::new();

    let pic1 = graph.add_node(TargetKind::Color, size2(400, 200), AllocKind::Dynamic, &[]);
    let ds1 = graph.add_node(TargetKind::Color, size2(200, 100), AllocKind::Dynamic, &[pic1]);
    let ds2 = graph.add_node(TargetKind::Color, size2(100, 50), AllocKind::Dynamic, &[ds1]);
    let vblur1 = graph.add_node(TargetKind::Color, size2(400, 300), AllocKind::Dynamic, &[ds2]);
    let hblur1 = graph.add_node(TargetKind::Color, size2(500, 300), AllocKind::Dynamic, &[vblur1]);

    let vblur2 = graph.add_node(TargetKind::Color, size2(400, 350), AllocKind::Fixed(TextureId(1337)), &[ds2]);
    let hblur2 = graph.add_node(TargetKind::Color, size2(550, 350), AllocKind::Dynamic, &[vblur2]);

    let vblur3 = graph.add_node(TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[ds1]);
    let hblur3 = graph.add_node(TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[vblur3]);

    let ds3 = graph.add_node(TargetKind::Color, size2(100, 50), AllocKind::Dynamic, &[ds2]);
    let vblur4 = graph.add_node(TargetKind::Color, size2(500, 400), AllocKind::Dynamic, &[ds3]);
    let hblur4 = graph.add_node(TargetKind::Color, size2(600, 400), AllocKind::Dynamic, &[vblur4]);

    let root = graph.add_node(TargetKind::Color, size2(1000, 1000),
        AllocKind::Fixed(TextureId(123)),
        &[pic1, hblur1, hblur2, hblur3, hblur4]
    );
    graph.add_root(root);

    for &with_deallocations in &[false, true] {
        for &pass_option in &[PassOptions::Recursive] {
            for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
                build_and_print_graph(
                    &graph,
                    BuilderOptions {
                        passes: pass_option,
                        targets: target_option,
                        culling: true,
                    },
                    with_deallocations,
                )
            }
        }
    }
}

