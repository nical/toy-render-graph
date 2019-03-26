use crate::{DeviceIntRect, DeviceIntSize};
use euclid::{size2, vec2};

type AllocId = usize;
const INVALID_ALLOC: AllocId = std::usize::MAX;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

impl Orientation {
    fn flipped(self) -> Self {
        match self {
            Orientation::Vertical => Orientation::Horizontal,
            Orientation::Horizontal => Orientation::Vertical,
        }
    }
}


#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Container,
    Alloc,
    Free,
    Unused,
}

#[derive(Clone, Debug)]
pub struct Node {
    parent: AllocId,
    next_sibbling: AllocId,
    prev_sibbling: AllocId,
    kind: NodeKind,
    orientation: Orientation,
    rect: DeviceIntRect,
}

pub struct Allocator {
    nodes: Vec<Node>,
    free_list: Vec<AllocId>,
    unused_nodes: AllocId,
}

fn fit(slot: &DeviceIntRect, size: &DeviceIntSize) -> bool {
    slot.size.width >= size.width && slot.size.height >= size.height
}

impl Allocator {
    pub fn new(size: DeviceIntSize) -> Self {
        Allocator {
            nodes: vec![Node {
                parent: INVALID_ALLOC,
                next_sibbling: INVALID_ALLOC,
                prev_sibbling: INVALID_ALLOC,
                rect: size.into(),
                kind: NodeKind::Free,
                orientation: Orientation::Vertical,
            }],
            free_list: vec![0],
            unused_nodes: INVALID_ALLOC,
        }
    }

    pub fn allocate(&mut self, requested_size: &DeviceIntSize) -> Option<AllocId> {

        // Find a suitable free rect.
        let chosen_id = self.find_suitable_rect(requested_size);

        if chosen_id == INVALID_ALLOC {
            //println!("failed to allocate {:?}", *requested_size);
            //self.print_free_rects();

            // No suitable free rect!
            return None;
        }

        // TODO: "round up" the allocated rect to avoid very thin split/leftover rects.

        let chosen_node = self.nodes[chosen_id].clone();
        let current_orientation = chosen_node.orientation;
        assert_eq!(chosen_node.kind, NodeKind::Free);

        // Decide wehther to split horizontally or vertically.

        let candidate_free_rect_to_right = DeviceIntRect {
            origin: chosen_node.rect.origin + vec2(requested_size.width, 0),
            size: size2(
                chosen_node.rect.size.width - requested_size.width,
                requested_size.height,
            ),
        };
        let candidate_free_rect_to_bottom = DeviceIntRect {
            origin: chosen_node.rect.origin + vec2(0, requested_size.height),
            size: size2(
                requested_size.width,
                chosen_node.rect.size.height - requested_size.height,
            ),
        };

        let allocated_rect = DeviceIntRect {
            origin: chosen_node.rect.origin,
            size: *requested_size,
        };

        let split_rect;
        let leftover_rect;
        let orientation;
        if *requested_size == chosen_node.rect.size {
            // Perfect fit.
            orientation = current_orientation;
            split_rect = DeviceIntRect::zero();
            leftover_rect = DeviceIntRect::zero();
        } else if candidate_free_rect_to_right.size.area() > candidate_free_rect_to_bottom.size.area() {
            leftover_rect = candidate_free_rect_to_bottom;
            split_rect = DeviceIntRect {
                origin: candidate_free_rect_to_right.origin,
                size: size2(candidate_free_rect_to_right.size.width, chosen_node.rect.size.height),
            };
            orientation = Orientation::Horizontal;
        } else {
            leftover_rect = candidate_free_rect_to_right;
            split_rect = DeviceIntRect {
                origin: candidate_free_rect_to_bottom.origin,
                size: size2(chosen_node.rect.size.width, candidate_free_rect_to_bottom.size.height),
            };
            orientation = Orientation::Vertical;
        }

        // Update the tree

        let allocated_id;
        let split_id;
        let leftover_id;
        //println!("{:?} -> {:?}", current_orientation, orientation);
        if orientation == current_orientation {
            if split_rect.size.area() > 0 {
                let next_sibbling = chosen_node.next_sibbling;

                split_id = self.new_node();
                self.nodes[split_id] = Node {
                    parent: chosen_node.parent,
                    next_sibbling,
                    prev_sibbling: chosen_id,
                    rect: split_rect,
                    kind: NodeKind::Free,
                    orientation: current_orientation,
                };

                self.nodes[chosen_id].next_sibbling = split_id;
                if next_sibbling != INVALID_ALLOC {
                    self.nodes[next_sibbling].prev_sibbling = split_id;
                }
            } else {
                split_id = INVALID_ALLOC;
            }

            if leftover_rect.size.area() > 0 {
                self.nodes[chosen_id].kind = NodeKind::Container;

                allocated_id = self.new_node();
                leftover_id = self.new_node();

                self.nodes[allocated_id] = Node {
                    parent: chosen_id,
                    next_sibbling: leftover_id,
                    prev_sibbling: INVALID_ALLOC,
                    rect: allocated_rect,
                    kind: NodeKind::Alloc,
                    orientation: current_orientation.flipped(),
                };

                self.nodes[leftover_id] = Node {
                    parent: chosen_id,
                    next_sibbling: INVALID_ALLOC,
                    prev_sibbling: allocated_id,
                    rect: leftover_rect,
                    kind: NodeKind::Free,
                    orientation: current_orientation.flipped(),
                };
            } else {
                // No need to split for the leftover area, we can allocate directly in the chosen node.
                allocated_id = chosen_id;
                let node = &mut self.nodes[chosen_id];
                node.kind = NodeKind::Alloc;
                node.rect = allocated_rect;

                leftover_id = INVALID_ALLOC
            }
        } else {
            self.nodes[chosen_id].kind = NodeKind::Container;

            if split_rect.size.area() > 0 {
                split_id = self.new_node();
                self.nodes[split_id] = Node {
                    parent: chosen_id,
                    next_sibbling: INVALID_ALLOC,
                    prev_sibbling: INVALID_ALLOC,
                    rect: split_rect,
                    kind: NodeKind::Free,
                    orientation: current_orientation.flipped(),
                };
            } else {
                split_id = INVALID_ALLOC;
            }

            if leftover_rect.size.area() > 0 {
                let container_id = self.new_node();
                self.nodes[container_id] = Node {
                    parent: chosen_id,
                    next_sibbling: split_id,
                    prev_sibbling: INVALID_ALLOC,
                    rect: DeviceIntRect::zero(),
                    kind: NodeKind::Container,
                    orientation: current_orientation.flipped(),
                };

                self.nodes[split_id].prev_sibbling = container_id;

                allocated_id = self.new_node();
                leftover_id = self.new_node();

                self.nodes[allocated_id] = Node {
                    parent: container_id,
                    next_sibbling: leftover_id,
                    prev_sibbling: INVALID_ALLOC,
                    rect: allocated_rect,
                    kind: NodeKind::Alloc,
                    orientation: current_orientation,
                };

                self.nodes[leftover_id] = Node {
                    parent: container_id,
                    next_sibbling: INVALID_ALLOC,
                    prev_sibbling: allocated_id,
                    rect: leftover_rect,
                    kind: NodeKind::Free,
                    orientation: current_orientation,
                };
            } else {
                allocated_id = self.new_node();
                self.nodes[allocated_id] = Node {
                    parent: chosen_id,
                    next_sibbling: split_id,
                    prev_sibbling: INVALID_ALLOC,
                    rect: allocated_rect,
                    kind: NodeKind::Alloc,
                    orientation: current_orientation.flipped(),
                };

                self.nodes[split_id].prev_sibbling = allocated_id;

                leftover_id = INVALID_ALLOC;
            }
        }

        if split_id != INVALID_ALLOC {
            self.add_free_rect(split_id, &split_rect.size);
        }

        if leftover_id != INVALID_ALLOC {
            self.add_free_rect(leftover_id, &leftover_rect.size);
        }

        //println!("allocated {:?}     split: {:?} leftover: {:?}", allocated_rect, split_rect, leftover_rect);
        //self.print_free_rects();

        #[cfg(feature = "checks")]
        self.check_tree();

        Some(allocated_id)
    }

    pub fn deallocate(&mut self, mut node_id: AllocId) {
        assert!(node_id < self.nodes.len());
        assert_eq!(self.nodes[node_id].kind, NodeKind::Alloc);

        //println!("deallocate rect {} #{:?}", self.nodes[node_id].rect, node_id);
        self.nodes[node_id].kind = NodeKind::Free;

        loop {
            let orientation = self.nodes[node_id].orientation;

            let next = self.nodes[node_id].next_sibbling;
            let prev = self.nodes[node_id].prev_sibbling;

            // Try to merge with the next node.
            if next != INVALID_ALLOC && self.nodes[next].kind == NodeKind::Free {
                self.merge_sibblings(node_id, next, orientation);
            }

            // Try to merge with the previous node.
            if prev != INVALID_ALLOC && self.nodes[prev].kind == NodeKind::Free {
                self.merge_sibblings(prev, node_id, orientation);
                node_id = prev;
            }

            // If this node is now a unique child. We collapse it into its parent and try to merge
            // again at the parent level.
            let parent = self.nodes[node_id].parent;
            if self.nodes[node_id].prev_sibbling == INVALID_ALLOC
                && self.nodes[node_id].next_sibbling == INVALID_ALLOC
                && parent != INVALID_ALLOC {
                //println!("collapse #{:?} into parent #{:?}", node_id, parent);

                self.mark_node_unused(node_id);

                // Replace the parent container with a free node.
                self.nodes[parent].rect = self.nodes[node_id].rect;
                self.nodes[parent].kind = NodeKind::Free;

                // Start again at the parent level.
                node_id = parent;
            } else {

                let size = self.nodes[node_id].rect.size;
                self.add_free_rect(node_id, &size);
                break;
            }
        }

        #[cfg(feature = "checks")]
        self.check_tree();
    }

    fn find_suitable_rect(&mut self, requested_size: &DeviceIntSize) -> AllocId {

        let mut i = 0;
        while i < self.free_list.len() {
            let id = self.free_list[i];

            // During tree simplification we don't remove merged nodes from the free list, so we have
            // to handle it here.
            // This is a tad awkward, but lets us avoid having to maintain a doubly linked list for
            // the free list (which would be needed to remove nodes during tree simplification).
            let should_remove = self.nodes[id].kind != NodeKind::Free;
            let suitable = !should_remove && fit(&self.nodes[id].rect, requested_size);

            if should_remove || suitable {
                // remove the element from the free list
                self.free_list.swap_remove(i);
            } else {
                i += 1;
            }

            if suitable {
                return id;
            }
        }

        INVALID_ALLOC
    }

    fn new_node(&mut self) -> AllocId {
        let idx = self.unused_nodes;
        if idx < self.nodes.len() {
            self.unused_nodes = self.nodes[idx].next_sibbling;
            return idx;
        }

        self.nodes.push(Node {
            parent: INVALID_ALLOC,
            next_sibbling: INVALID_ALLOC,
            prev_sibbling: INVALID_ALLOC,
            rect: DeviceIntRect::zero(),
            kind: NodeKind::Unused,
            orientation: Orientation::Horizontal,
        });

        self.nodes.len() - 1
    }

    fn mark_node_unused(&mut self, id: AllocId) {
        debug_assert!(self.nodes[id].kind != NodeKind::Unused);
        self.nodes[id].kind = NodeKind::Unused;
        self.nodes[id].next_sibbling = self.unused_nodes;
        self.unused_nodes = id;
    }

    #[cfg(feature = "checks")]
    fn print_free_rects(&self) {
        for &id in &self.free_list {
            if self.nodes[id].kind == NodeKind::Free {
                //println!(" - {} #{}", self.nodes[id].rect, id);
            }
        }
    }

    #[cfg(feature = "checks")]
    fn check_sibblings(&self, id: AllocId, next: AllocId, orientation: Orientation) {
        if next == INVALID_ALLOC {
            return;
        }

        if self.nodes[next].prev_sibbling != id {
            //println!("error: #{}'s next sibbling #{} has prev sibbling #{}", id, next, self.nodes[next].prev_sibbling);
        }
        assert_eq!(self.nodes[next].prev_sibbling, id);

        match self.nodes[id].kind {
            NodeKind::Container | NodeKind::Unused => {
                return;
            }
            _ => {}
        }
        match self.nodes[next].kind {
            NodeKind::Container | NodeKind::Unused => {
                return;
            }
            _ => {}
        }

        let r1 = self.nodes[id].rect;
        let r2 = self.nodes[next].rect;
        match orientation {
            Orientation::Horizontal => {
                assert_eq!(r1.min_y(), r1.min_y());
                assert_eq!(r1.max_y(), r1.max_y());
            }
            Orientation::Vertical => {
                assert_eq!(r1.min_x(), r1.min_x());
                assert_eq!(r1.max_x(), r1.max_x());
            }
        }
    }

    fn check_tree(&self) {
        for node_id in 0..self.nodes.len() {
            let node = &self.nodes[node_id];

            if node.kind == NodeKind::Unused {
                continue;
            }

            let mut iter = node.next_sibbling;
            while iter != INVALID_ALLOC {
                assert_eq!(self.nodes[iter].orientation, node.orientation);
                assert_eq!(self.nodes[iter].parent, node.parent);
                let next = self.nodes[iter].next_sibbling;

                #[cfg(feature = "checks")]
                self.check_sibblings(iter, next, node.orientation);

                iter = next;

            }

            if node.parent != INVALID_ALLOC {
                if self.nodes[node.parent].kind != NodeKind::Container {
                    //println!("error: child: {:?} parent: {:?}", node_id, node.parent);
                }
                assert_eq!(self.nodes[node.parent].orientation, node.orientation.flipped());
                assert_eq!(self.nodes[node.parent].kind, NodeKind::Container);
            }
        }
    }

    fn add_free_rect(&mut self, id: AllocId, _size: &DeviceIntSize) {
        //println!("add free rect #{}", id);
        // TODO: Separate small/medium /large free rect lists.
        debug_assert_eq!(self.nodes[id].kind, NodeKind::Free);
        self.free_list.push(id);
    }

    // Merge `next` into `node` and append `next` to a list of available `nodes`vector slots.
    fn merge_sibblings(&mut self, node: AllocId, next: AllocId, orientation: Orientation) {
        let r1 = self.nodes[node].rect;
        let r2 = self.nodes[next].rect;
        //println!("merge {} #{} and {} #{}       {:?}", r1, node, r2, next, orientation);
        let merge_size = self.nodes[next].rect.size;
        match orientation {
            Orientation::Horizontal => {
                assert_eq!(r1.min_y(), r2.min_y());
                assert_eq!(r1.max_y(), r2.max_y());
                self.nodes[node].rect.size.width += merge_size.width;
            }
            Orientation::Vertical => {
                assert_eq!(r1.min_x(), r2.min_x());
                assert_eq!(r1.max_x(), r2.max_x());
                self.nodes[node].rect.size.height += merge_size.height;
            }
        }

        // Remove the merged node from the sibbling list.
        let next_next = self.nodes[next].next_sibbling;
        self.nodes[node].next_sibbling = next_next;
        if next_next != INVALID_ALLOC {
            self.nodes[next_next].prev_sibbling = node;
        }

        // Add the merged node to the list of available slots in the nodes vector.
        self.mark_node_unused(next);
    }
}


#[test]
fn atlas_simple() {
    let mut atlas = Allocator::new(size2(1000, 1000));

    let full = atlas.allocate(&size2(1000,1000)).unwrap();
    assert!(atlas.allocate(&size2(1, 1)).is_none());

    atlas.deallocate(full);

    let a = atlas.allocate(&size2(100, 1000)).unwrap();
    let b = atlas.allocate(&size2(900, 200)).unwrap();
    let c = atlas.allocate(&size2(300, 200)).unwrap();
    let d = atlas.allocate(&size2(200, 300)).unwrap();
    let e = atlas.allocate(&size2(100, 300)).unwrap();
    let f = atlas.allocate(&size2(100, 300)).unwrap();
    let g = atlas.allocate(&size2(100, 300)).unwrap();

    atlas.deallocate(b);
    atlas.deallocate(f);
    atlas.deallocate(c);
    atlas.deallocate(e);
    let h = atlas.allocate(&size2(500, 200)).unwrap();
    atlas.deallocate(a);
    let i = atlas.allocate(&size2(500, 200)).unwrap();
    atlas.deallocate(g);
    atlas.deallocate(h);
    atlas.deallocate(d);
    atlas.deallocate(i);

    let full = atlas.allocate(&size2(1000,1000)).unwrap();
    assert!(atlas.allocate(&size2(1, 1)).is_none());
    atlas.deallocate(full);
}

#[test]
fn atlas_random_test() {
    let mut atlas = Allocator::new(size2(1000, 1000));

    let a = 1103515245;
    let c = 12345;
    let m = usize::pow(2, 31);
    let mut seed: usize = 37;

    let mut rand = || {
        seed = (a * seed + c) % m;
        seed
    };

    let mut n: usize = 0;
    let mut misses: usize = 0;

    let mut allocated = Vec::new();
    for _ in 0..1000000 {
        if rand() % 5 > 2 && !allocated.is_empty() {
            // deallocate something
            let nth = rand() % allocated.len();
            let id = allocated[nth];
            allocated.remove(nth);

            atlas.deallocate(id);
        } else {
            // allocate something
            let size = size2(
                (rand() % 300) as i32 + 5,
                (rand() % 300) as i32 + 5,
            );

            if let Some(id) = atlas.allocate(&size) {
                allocated.push(id);
                n += 1;
            } else {
                misses += 1;
            }
        }
    }

    while let Some(id) = allocated.pop() {
        atlas.deallocate(id);
    }

    println!("added/removed {} rectangles, {} misses", n, misses);
    println!("nodes.cap: {}, free_list.cap: {}", atlas.nodes.capacity(), atlas.free_list.capacity());

    let full = atlas.allocate(&size2(1000,1000)).unwrap();
    assert!(atlas.allocate(&size2(1, 1)).is_none());
    atlas.deallocate(full);
}


