use std::usize;
use std::i32;

use crate::AllocatedRect;

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn to_usize(self) -> usize { self.0 as usize }
}

pub(crate) fn node_id(idx: usize) -> NodeId {
    debug_assert!(idx < std::u32::MAX as usize);
    NodeId(idx as u32)
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TargetId(pub(crate) u32);

impl TargetId {
    pub fn to_usize(self) -> usize { self.0 as usize }
}

pub(crate) fn target_id(idx: usize) -> TargetId {
    debug_assert!(idx < std::u32::MAX as usize);
    TargetId(idx as u32)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AllocId(TargetId, u32);

pub trait AtlasAllocator {
    fn allocate(&mut self, target: TargetId, size: DeviceIntSize) -> AllocatedRect;
    fn deallocate(&mut self, id: AllocId);
}

pub struct DummyAtlasAllocator { n: u32 }

impl DummyAtlasAllocator {
    pub fn new() -> Self {
        DummyAtlasAllocator { n: 0 }
    }
}

impl AtlasAllocator for DummyAtlasAllocator {
    fn allocate(&mut self, target: TargetId, _size: DeviceIntSize) -> AllocatedRect {
        self.n += 1;
        AllocatedRect {
            alloc_id: AllocId(target, self.n),
            rect: DeviceIntBox2D::zero()
        }
    }

    fn deallocate(&mut self, _id: AllocId) {}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntBox2D {
    pub min: DeviceIntPoint,
    pub max: DeviceIntPoint,
}

impl DeviceIntBox2D {
    pub fn zero() -> Self {
        DeviceIntBox2D {
            min: DeviceIntPoint { x: 0, y: 0 },
            max: DeviceIntPoint { x: 0, y: 0 },
        }
    }
}

pub fn size2(width: i32, height: i32) -> DeviceIntSize {
    DeviceIntSize { width, height }
}

