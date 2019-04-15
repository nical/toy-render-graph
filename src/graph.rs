
use std::i32;
use std::ops::Range;

pub use guillotiere::{Rectangle, Size, Point};
pub use euclid::{size2, vec2, point2};

use crate::allocator::*;
pub use crate::allocator::{TextureId, TextureAllocator, GuillotineAllocator, DbgTextureAllocator};

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub fn index(self) -> usize { self.0 as usize }
}

fn node_id(idx: usize) -> NodeId {
    debug_assert!(idx < std::u32::MAX as usize);
    NodeId(idx as u32)
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NodeIdRange {
    start: u32,
    end: u32,
}

impl Iterator for NodeIdRange {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.start >= self.end {
            return None;
        }

        let result = Some(NodeId(self.start));
        self.start += 1;

        result
    }
}

impl NodeIdRange {
    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn get(&self, nth: usize) -> NodeId {
        assert!(nth < self.len());
        NodeId(self.start + nth as u32)
    }
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
    pub dependencies: Range<u32>,
    pub target_kind: TargetKind,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AllocKind {
    Fixed(TextureId, Point),
    Dynamic,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Blit,
    Render(u64),
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Graph {
    nodes: Vec<Node>,
    dependencies: Vec<NodeId>,
    roots: Vec<NodeId>,
}

fn usize_range(u32_range: &Range<u32>) -> Range<usize> {
    (u32_range.start as usize) .. (u32_range.end as usize)
}

impl Graph {
    pub fn new() -> Self {
        Graph {
            nodes: Vec::new(),
            dependencies: Vec::new(),
            roots: Vec::new(),
        }
    }

    pub fn add_node(&mut self, task_kind: TaskKind, target_kind: TargetKind, size: Size, alloc_kind: AllocKind, deps: &[NodeId]) -> NodeId {
        let dep_start = self.dependencies.len() as u32;
        self.dependencies.extend_from_slice(deps);
        let dep_end = self.dependencies.len() as u32;

        let id = node_id(self.nodes.len());
        self.nodes.push(Node {
            task_kind,
            size,
            alloc_kind,
            dependencies: dep_start..dep_end,
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

    pub fn node_ids(&self) -> NodeIdRange {
        NodeIdRange {
            start: 0,
            end: self.nodes.len() as u32,
        }
    }

    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn node_dependencies(&self, node: NodeId) -> &[NodeId] {
        let range = self.nodes[node.index()].dependencies.clone();
        &self.dependencies[usize_range(&range)]
    }
}

impl std::ops::Index<NodeId> for Graph {
    type Output = Node;
    fn index(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct BuiltGraph {
    graph: Graph,
    pub passes: Vec<Pass>,
}

impl std::ops::Deref for BuiltGraph {
    type Target = Graph;
    fn deref(&self) -> &Graph {
        &self.graph
    }
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct Pass {
    pub dynamic_targets: [PassTarget; NUM_TARGET_KINDS],
    pub fixed_targets: Vec<PassTarget>,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Task {
    pub node: NodeId,
    pub rectangle: Rectangle,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct PassTarget {
    pub(crate) tasks: Vec<Task>,
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
pub enum TargetOptions {
    Direct,
    PingPong,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BuilderOptions {
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

        create_passes(
            &graph,
            &mut active_nodes,
            &mut passes,
            &mut node_passes,
        );

        // Step 4 - assign render targets to passes.
        //
        // A render target can be used by several passes as long as no pass
        // both read and write the same render target.

        match self.options.targets {
            TargetOptions::Direct => assign_targets_direct(
                &mut graph,
                &mut passes,
                &mut node_passes,
                allocator,
            ),
            TargetOptions::PingPong => assign_targets_ping_pong(
                &mut graph,
                &mut passes,
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
            &mut passes,
            &mut active_nodes,
            &mut allocated_rects,
            allocator,
        );

        BuiltGraph {
            graph: graph,
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
    fn mark_active_node(graph: &Graph, id: NodeId, active_nodes: &mut [bool]) {
        let idx = id.index();
        if active_nodes[idx] {
            return;
        }

        active_nodes[idx] = true;

        for &dep in graph.node_dependencies(id) {
            mark_active_node(graph, dep, active_nodes);
        }
    }

    reset(active_nodes, graph.nodes.len(), false);

    for &root in &graph.roots {
        mark_active_node(graph, root, active_nodes);
    }
}

/// Create render passes and assign the nodes to them.
///
/// This method tries to emulate WebRender's current behavior.
/// Nodes are executed as late as possible.
fn create_passes(
    graph: &Graph,
    active_nodes: &[bool],
    passes: &mut Vec<Pass>,
    node_passes: &mut [i32],
) {
    fn assign_depth(
        graph: &Graph,
        node_id: NodeId,
        rev_pass_index: usize,
        node_rev_passes: &mut [usize],
        max_depth: &mut usize,
    ) {
        *max_depth = std::cmp::max(*max_depth, rev_pass_index);

        node_rev_passes[node_id.index()] = std::cmp::max(
            node_rev_passes[node_id.index()],
            rev_pass_index,
        );

        for &dep in graph.node_dependencies(node_id) {
            assign_depth(
                graph,
                dep,
                rev_pass_index + 1,
                node_rev_passes,
                max_depth,
            );
        }
    }

    let mut max_depth = 0;
    let mut node_rev_passes = vec![0; graph.nodes.len()];

    for &root in &graph.roots {
        assign_depth(
            &graph,
            root,
            0,
            &mut node_rev_passes,
            &mut max_depth,
        );
    }

    for _ in 0..(max_depth + 1) {
        passes.push(Pass {
            dynamic_targets: [
                PassTarget {
                    tasks: Vec::new(),
                    destination: None,
                },
                PassTarget {
                    tasks: Vec::new(),
                    destination: None,
                },
            ],
            fixed_targets: Vec::new(),
        });
    }

    for id in graph.node_ids() {
        let node_idx = id.index();
        if !active_nodes[node_idx] {
            continue;
        }

        let target_kind = graph.nodes[node_idx].target_kind;
        let pass_index = max_depth - node_rev_passes[node_idx];
        match graph.nodes[node_idx].alloc_kind {
            AllocKind::Dynamic => {
                passes[pass_index].dynamic_targets[target_kind as usize].tasks.push(Task {
                    node: id,
                    rectangle: Rectangle::zero(), // Will be set in allocate_target_rects.
                });
            }
            AllocKind::Fixed(texture_id, origin) => {
                let size = graph.nodes[node_idx].size;
                let task = Task {
                    node: id,
                    rectangle: Rectangle {
                        min: origin,
                        max: origin + size.to_vector(),
                    },
                };
                let mut new_target = true;
                for target in &mut passes[pass_index].fixed_targets {
                    if target.destination == Some(texture_id) {
                        new_target = false;
                        target.tasks.push(task);
                        break;
                    }
                }
                if new_target {
                    passes[pass_index].fixed_targets.push(PassTarget {
                        tasks: vec![task],
                        destination: Some(texture_id),
                    });
                }
            }
        }
        node_passes[node_idx] = pass_index as i32;
    }
}

/// Assign a render target to each pass with a "ping-pong" scheme alternating between
/// two render targets.
///
/// In order to ensure that a node never reads and writes from the same target, some
/// blit nodes may be inserted in the graph.
fn assign_targets_ping_pong(
    graph: &mut Graph,
    passes: &mut[Pass],
    node_passes: &mut [i32],
    allocator: &mut dyn TextureAllocator,
) {
    let mut node_redirects = vec![None; graph.nodes.len()];

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

    let mut nth_dynamic_pass = [
        // color
        0,
        // alpha
        0,
    ];

    for p in 0..passes.len() {
        for target_kind_index in 0..NUM_TARGET_KINDS {
            if passes[p].dynamic_targets[target_kind_index].tasks.is_empty() {
                continue;
            }

            let ping_pong = nth_dynamic_pass[target_kind_index] % 2;
            nth_dynamic_pass[target_kind_index] += 1;

            let current_destination = texture_ids[target_kind_index][ping_pong];
            passes[p].dynamic_targets[target_kind_index].destination = Some(current_destination);

            for nth_node in 0..passes[p].dynamic_targets[target_kind_index].tasks.len() {
                let node = passes[p].dynamic_targets[target_kind_index].tasks[nth_node].node;
                for dep_location in usize_range(&graph.nodes[node.index()].dependencies) {
                    let dep = graph.dependencies[dep_location];
                    let dep_pass = node_passes[dep.index()] as usize;
                    let dep_target_kind = graph.nodes[dep.index()].target_kind;

                    // Can't both read and write the same target.
                    if passes[dep_pass].dynamic_targets[dep_target_kind as usize].destination == Some(current_destination) {
                        graph.dependencies[dep_location] = handle_conflict_using_bit_task(
                            graph,
                            passes,
                            &mut node_redirects,
                            dep,
                            dep_target_kind,
                            p
                        );
                    }
                }
            }
        }
    }
}

fn handle_conflict_using_bit_task(
    graph: &mut Graph,
    passes: &mut[Pass],
    node_redirects: &mut[Option<NodeId>],
    dep: NodeId,
    dep_target_kind: TargetKind,
    pass: usize,
) -> NodeId {
    // See if we have already added a blit task to avoid the problem.
    if let Some(source) = node_redirects[dep.index()] {
        return source;
    }

    // Otherwise add a blit task.
    let blit_id = node_id(graph.nodes.len());
    let size = graph.nodes[dep.index()].size;
    let target_kind = graph.nodes[dep.index()].target_kind;
    let blit_dep_index = graph.dependencies.len() as u32;
    graph.dependencies.push(dep);
    graph.nodes.push(Node {
        task_kind: TaskKind::Blit,
        dependencies: blit_dep_index..(blit_dep_index + 1),
        alloc_kind: AllocKind::Dynamic,
        size,
        target_kind,
    });
    node_redirects[dep.index()] = Some(blit_id);

    passes[pass - 1]
        .dynamic_targets[dep_target_kind as usize]
        .tasks
        .push(Task {
            node: blit_id,
            rectangle: Rectangle::zero(),
        });

    blit_id
}

/// Assign a render target to each pass without adding nodes to the graph.
///
/// This method may generate more render targets than assign_targets_ping_pong,
/// however it doesn't add any extra copying operations.
fn assign_targets_direct(
    graph: &mut Graph,
    passes: &mut[Pass],
    node_passes: &mut [i32],
    allocator: &mut dyn TextureAllocator,
) {
    let mut allocated_textures = [Vec::new(), Vec::new()];
    let mut dependencies = std::collections::HashSet::new();

    for p in 0..passes.len() {
        let pass = &passes[p];

        dependencies.clear();
        for target in &pass.dynamic_targets {
            for task in &target.tasks {
                for &dep in graph.node_dependencies(task.node) {
                    let dep_pass = node_passes[dep.index()];
                    let target_kind = graph.nodes[dep.index()].target_kind;
                    if let Some(id) = passes[dep_pass as usize].dynamic_targets[target_kind as usize].destination {
                        dependencies.insert(id);
                    }
                }
            }
        }

        for target_kind_index in 0..NUM_TARGET_KINDS {
            if passes[p].dynamic_targets[target_kind_index].tasks.is_empty() {
                continue;
            }

            let mut destination = None;
            for target_id in &allocated_textures[target_kind_index] {
                if !dependencies.contains(target_id) {
                    destination = Some(*target_id);
                    break;
                }
            }

            let destination = destination.unwrap_or_else(|| {
                let id = allocator.add_texture();
                allocated_textures[target_kind_index].push(id);
                id
            });

            passes[p].dynamic_targets[target_kind_index].destination = Some(destination);
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
    passes: &mut[Pass],
    visited: &mut[bool],
    allocated_rects: &mut Vec<Option<AllocId>>,
    allocator: &mut dyn TextureAllocator,
) {
    let mut last_node_refs: Vec<NodeId> = Vec::with_capacity(graph.nodes.len());
    let mut pass_last_node_ranges: Vec<std::ops::Range<usize>> = vec![0..0; passes.len()];

    // The first step is to find for each pass the list of nodes that are not referenced
    // anymore after the pass ends.

    // Mark roots as visited to avoid deallocating their target rects.
    for root in &graph.roots {
        visited[root.index()] = true;
    }

    // Visit passes in reverse order and look at the dependencies.
    // Each dependency that we haven't visited yet is the last reference to a node.
    let mut pass_index = passes.len();
    for pass in passes.iter().rev() {
        pass_index -= 1;
        let first = last_node_refs.len();
        for target_kind in 0..NUM_TARGET_KINDS {
            for task in &pass.dynamic_targets[target_kind].tasks {
                for &dep in graph.node_dependencies(task.node) {
                    let dep_idx = dep.index();
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
    for (pass_index, pass) in passes.iter_mut().enumerate() {
        for pass_target in &mut pass.dynamic_targets {
            if pass_target.tasks.is_empty() {
                continue;
            }
            let texture = pass_target.destination.unwrap();

            // Allocations needed for this pass.
            for task in &mut pass_target.tasks {
                let node_idx = task.node.index();
                let node = &graph.nodes[node_idx];
                let size = graph.nodes[node_idx].size;
                match node.alloc_kind {
                    AllocKind::Dynamic => {
                        let alloc = allocator.allocate(texture, size);
                        task.rectangle = alloc.rectangle;
                        allocated_rects[node_idx] = Some(alloc.id);
                    }
                    AllocKind::Fixed(_, origin) => {
                        task.rectangle = Rectangle {
                            min: origin,
                            max: origin + size.to_vector(),
                        }
                    }
                }
            }
        }

        // Deallocations we can perform after this pass.
        let finished_range = pass_last_node_ranges[pass_index].clone();
        for finished_node in &last_node_refs[finished_range] {
            let node_idx = finished_node.index();
            if let Some(alloc_id) = allocated_rects[node_idx] {
                allocator.deallocate(alloc_id);
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
        for target in &pass.dynamic_targets {
            n_nodes += target.tasks.len();
        }
        for target in &pass.fixed_targets {
            n_nodes += target.tasks.len();
        }
    }

    println!(
        "\n\n------------- deallocations: {:?}, culling: {:?}, targets: {:?}",
        with_deallocations,
        options.culling,
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
            if let Some(texture) = pass.dynamic_targets[target_kind as usize].destination {
                println!("  * Dynamic {:?} target {:?}:", target_kind, texture);
                for task in &pass.dynamic_targets[target_kind as usize].tasks {
                    let r = &task.rectangle;
                    println!("     - {:?} {:?}      rect: [({}, {}) {}x{}]",
                        task.node,
                        built_graph.nodes[task.node.index()].task_kind,
                        r.min.x, r.min.y, r.size().width, r.size().height,
                    );
                }
            }
        }

        for target in &pass.fixed_targets {
            println!("  * Fixed target {:?}:", target.destination.unwrap());
            for task in &target.tasks {
                let r = &task.rectangle;
                println!("     - {:?} {:?}      rect: [({}, {}) {}x{}]",
                    task.node,
                    built_graph.nodes[task.node.index()].task_kind,
                    r.min.x, r.min.y, r.size().width, r.size().height,
                );
            }
        }
    }
}


#[test]
fn simple_graph() {
    let mut graph = Graph::new();

    let n0 = graph.add_node(TaskKind::Render(0), TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[]);
    let _ = graph.add_node(TaskKind::Render(1), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n0]);
    let n2 = graph.add_node(TaskKind::Render(2), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[]);
    let n3 = graph.add_node(TaskKind::Render(3), TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[]);
    let n4 = graph.add_node(TaskKind::Render(4), TargetKind::Alpha, size2(100, 100), AllocKind::Dynamic, &[n2, n3]);
    let n5 = graph.add_node(TaskKind::Render(5), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[]);
    let n6 = graph.add_node(TaskKind::Render(6), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n3, n5]);
    let n7 = graph.add_node(TaskKind::Render(7), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[n2, n4, n6]);
    let n8 = graph.add_node(TaskKind::Render(8), TargetKind::Color, size2(800, 600), AllocKind::Fixed(TextureId(100), point2(0, 0)), &[n7]);

    graph.add_root(n5);
    graph.add_root(n8);

    for &with_deallocations in &[true] {
        for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
            build_and_print_graph(
                &graph,
                BuilderOptions {
                    targets: target_option,
                    culling: true,
                },
                with_deallocations,
            )
        }
    }
}

#[test]
fn test_stacked_shadows() {
    let mut graph = Graph::new();

    let pic1 = graph.add_node(TaskKind::Render(0), TargetKind::Color, size2(400, 200), AllocKind::Dynamic, &[]);
    let ds1 = graph.add_node(TaskKind::Render(1), TargetKind::Color, size2(200, 100), AllocKind::Dynamic, &[pic1]);
    let ds2 = graph.add_node(TaskKind::Render(2), TargetKind::Color, size2(100, 50), AllocKind::Dynamic, &[ds1]);
    let vblur1 = graph.add_node(TaskKind::Render(3), TargetKind::Color, size2(400, 300), AllocKind::Dynamic, &[ds2]);
    let hblur1 = graph.add_node(TaskKind::Render(4), TargetKind::Color, size2(500, 300), AllocKind::Dynamic, &[vblur1]);

    let vblur2 = graph.add_node(TaskKind::Render(5), TargetKind::Color, size2(400, 350), AllocKind::Fixed(TextureId(1337), point2(0, 0)), &[ds2]);
    let hblur2 = graph.add_node(TaskKind::Render(6), TargetKind::Color, size2(550, 350), AllocKind::Dynamic, &[vblur2]);

    let vblur3 = graph.add_node(TaskKind::Render(7), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[ds1]);
    let hblur3 = graph.add_node(TaskKind::Render(8), TargetKind::Color, size2(100, 100), AllocKind::Dynamic, &[vblur3]);

    let ds3 = graph.add_node(TaskKind::Render(9), TargetKind::Color, size2(100, 50), AllocKind::Dynamic, &[ds2]);
    let vblur4 = graph.add_node(TaskKind::Render(10), TargetKind::Color, size2(500, 400), AllocKind::Dynamic, &[ds3]);
    let hblur4 = graph.add_node(TaskKind::Render(11), TargetKind::Color, size2(600, 400), AllocKind::Dynamic, &[vblur4]);

    let root = graph.add_node(
        TaskKind::Render(0),
        TargetKind::Color,
        size2(1000, 1000),
        AllocKind::Fixed(TextureId(123), point2(0, 0)),
        &[pic1, hblur1, hblur2, hblur3, hblur4]
    );
    graph.add_root(root);

    for &with_deallocations in &[false, true] {
        for &target_option in &[TargetOptions::Direct, TargetOptions::PingPong] {
            build_and_print_graph(
                &graph,
                BuilderOptions {
                    targets: target_option,
                    culling: true,
                },
                with_deallocations,
            )
        }
    }
}

