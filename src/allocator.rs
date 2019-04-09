use std::usize;
use std::collections::HashSet;
use crate::{Size, Rectangle};

pub use guillotiere::{AtlasAllocator, Allocation, AllocId as RectangleId, AllocatorOptions};

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);

impl TextureId {
    pub fn index(self) -> usize { self.0 as usize }
}

pub(crate) fn texture_id(idx: usize) -> TextureId {
    debug_assert!(idx < std::u32::MAX as usize);
    TextureId(idx as u32)
}


#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AllocatedRectangle {
    pub rectangle: Rectangle,
    pub id: AllocId,
}

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AllocId {
    pub texture: TextureId,
    pub slice: u32,
    pub rectangle: RectangleId,
}

pub struct TextureArray {
    slices: Vec<AtlasAllocator>,
    size: Size,
    id: TextureId,
    options: AllocatorOptions,
}

impl TextureArray {
    pub fn new(id: TextureId, size: Size) -> Self {
        TextureArray {
            slices: Vec::new(),
            size,
            id,
            options: guillotiere::DEFAULT_OPTIONS,
        }
    }

    pub fn allocate(&mut self, size: Size) -> AllocatedRectangle {
        if self.size.width < size.width || self.size.height < size.height {
            self.resize(size);
        }

        if let Some(slice) = self.slices.last_mut() {
            if let Some(alloc) = slice.allocate(size) {
                return AllocatedRectangle {
                    rectangle: alloc.rectangle,
                    id: AllocId {
                        texture: self.id,
                        slice: self.slices.len() as u32,
                        rectangle: alloc.id,
                    }
                };
            }
        }

        for slice in &mut self.slices {
            if let Some(alloc) = slice.allocate(size) {
                return AllocatedRectangle {
                    rectangle: alloc.rectangle,
                    id: AllocId {
                        texture: self.id,
                        slice: self.slices.len() as u32,
                        rectangle: alloc.id,
                    },
                };
            }
        }

        self.slices.push(AtlasAllocator::with_options(self.size, &self.options));

        let alloc = self.slices.last_mut().unwrap().allocate(size).unwrap();

        AllocatedRectangle {
            rectangle: alloc.rectangle,
            id: AllocId {
                texture: self.id,
                slice: self.slices.len() as u32,
                rectangle: alloc.id,
            },
        }
    }

    pub fn deallocate(&mut self, id: AllocId) {
        assert_eq!(self.id, id.texture);
        self.slices[id.slice as usize].deallocate(id.rectangle);
    }

    pub fn resize(&mut self, mut new_size: Size) {
        new_size.width = new_size.width.max(self.size.width);
        new_size.height = new_size.height.max(self.size.height);
        for slice in &mut self.slices {
            slice.grow(new_size);
        }
    }

    pub fn num_slices(&self) -> usize {
        self.slices.len()
    }

    pub fn texture_size(&self) -> Size {
        self.size
    }
}

pub trait TextureAllocator {
    fn add_texture(&mut self) -> TextureId;
    fn allocate(&mut self, tex: TextureId, size: Size) -> AllocatedRectangle;
    fn deallocate(&mut self, id: AllocId);
}

pub struct GuillotineAllocator {
    pub textures: Vec<AtlasAllocator>,
    pub size: Size,
    pub options: AllocatorOptions,
}

impl GuillotineAllocator {
    pub fn new(size: Size) -> Self {
        GuillotineAllocator {
            textures: Vec::new(),
            size,
            options: guillotiere::DEFAULT_OPTIONS,
        }
    }

    pub fn with_options(size: Size, options: &AllocatorOptions) -> Self {
        GuillotineAllocator {
            textures: Vec::new(),
            size,
            options: options.clone(),
        }
    }
}

impl TextureAllocator for GuillotineAllocator {

    fn add_texture(&mut self) -> TextureId {
        self.textures.push(AtlasAllocator::with_options(self.size, &self.options));
        texture_id(self.textures.len() - 1)
    }

    fn allocate(&mut self, texture_id: TextureId, size: Size) -> AllocatedRectangle {
        let atlas = &mut self.textures[texture_id.index()];
        loop {
            if let Some(alloc) = atlas.allocate(size) {
                return AllocatedRectangle {
                    rectangle: alloc.rectangle,
                    id: AllocId {
                        texture: texture_id,
                        rectangle: alloc.id,
                        slice: 0,
                    }
                }
            }
            let new_size = atlas.size() * 2;
            atlas.grow(new_size);
        }
    }

    fn deallocate(&mut self, id: AllocId) {
        self.textures[id.texture.index()].deallocate(id.rectangle);
    }
}

pub struct DbgTextureAllocator<'l> {
    pub allocator: &'l mut dyn TextureAllocator,
    pub textures: Vec<HashSet<Rectangle>>,
    pub max_pixels: i32,
    pub max_rects: usize,
    pub record_deallocations: bool,
}

impl<'l> DbgTextureAllocator<'l> {
    pub fn new(allocator: &'l mut dyn TextureAllocator) -> Self {
        DbgTextureAllocator {
            allocator,
            textures: Vec::new(),
            max_pixels: 0,
            max_rects: 0,
            record_deallocations: true,
        }
    }

    pub fn max_allocated_pixels(&self) -> i32 { self.max_pixels }

    pub fn max_allocated_rects(&self) -> usize { self.max_rects }
}

impl<'l> TextureAllocator for DbgTextureAllocator<'l> {
    fn add_texture(&mut self) -> TextureId {
        self.textures.push(HashSet::new());
        self.allocator.add_texture()
    }

    fn allocate(&mut self, texture_id: TextureId, size: Size) -> AllocatedRectangle {
        let alloc = self.allocator.allocate(texture_id, size);

        self.textures[texture_id.index()].insert(alloc.rectangle);

        let mut pixels = 0;
        let mut rects = 0;
        for tex in &self.textures {
            rects += tex.len();
            for rect in tex {
                pixels += rect.area();
            }
        }

        self.max_pixels = std::cmp::max(self.max_pixels, pixels);
        self.max_rects = std::cmp::max(self.max_rects, rects);

        alloc
    }

    fn deallocate(&mut self, id: AllocId) {
        if self.record_deallocations {
            //self.textures[texture_id.index()].remove(&id);
            self.allocator.deallocate(id);
        }
    }
}

