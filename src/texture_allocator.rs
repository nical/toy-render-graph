/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::slice::Iter;
use crate::{DeviceIntPoint, DeviceIntRect, DeviceIntSize};

pub fn rect_is_empty(rect: &DeviceIntRect) -> bool {
    rect.size.width == 0 || rect.size.height == 0
}

pub type FastHashMap<K, V> = std::collections::HashMap<K, V>;

/// The minimum number of pixels on each side that we require for rects to be classified as
/// "medium" within the free list.
const MINIMUM_MEDIUM_RECT_SIZE: i32 = 16;

/// The minimum number of pixels on each side that we require for rects to be classified as
/// "large" within the free list.
const MINIMUM_LARGE_RECT_SIZE: i32 = 32;

enum CoalescingStatus {
    Changed,
    Unchanged,
}

/// A texture allocator using the guillotine algorithm with the rectangle merge improvement. See
/// sections 2.2 and 2.2.5 in "A Thousand Ways to Pack the Bin - A Practical Approach to Two-
/// Dimensional Rectangle Bin Packing":
///
///    http://clb.demon.fi/files/RectangleBinPack.pdf
///
/// This approach was chosen because of its simplicity, good performance, and easy support for
/// dynamic texture deallocation.
pub struct TexturePage {
    texture_size: DeviceIntSize,
    free_list: FreeRectList,
    coalesce_vec: Vec<DeviceIntRect>,
    allocations: u32,
    dirty: bool,
}

impl TexturePage {
    pub fn new(texture_size: DeviceIntSize) -> TexturePage {
        let mut page = TexturePage {
            texture_size,
            free_list: FreeRectList::new(),
            coalesce_vec: Vec::new(),
            allocations: 0,
            dirty: false,
        };
        page.clear();
        page
    }

    fn find_index_of_best_rect_in_bin(&self, bin: FreeListBin, requested_dimensions: &DeviceIntSize)
                                      -> Option<FreeListIndex> {
        let mut smallest_index_and_area = None;
        for (candidate_index, candidate_rect) in self.free_list.iter(bin).enumerate() {
            if !requested_dimensions.fits_inside(&candidate_rect.size) {
                continue
            }

            let candidate_area = candidate_rect.size.width * candidate_rect.size.height;
            smallest_index_and_area = Some((candidate_index, candidate_area));
            break
        }

        smallest_index_and_area.map(|(index, _)| FreeListIndex(bin, index))
    }

    /// Find a suitable rect in the free list. We choose the smallest such rect
    /// in terms of area (Best-Area-Fit, BAF).
    fn find_index_of_best_rect(&self, requested_dimensions: &DeviceIntSize)
                               -> Option<FreeListIndex> {
        let bin = FreeListBin::for_size(requested_dimensions);
        for &target_bin in &[FreeListBin::Small, FreeListBin::Medium, FreeListBin::Large] {
            if bin <= target_bin {
                if let Some(index) = self.find_index_of_best_rect_in_bin(target_bin,
                                                                         requested_dimensions) {
                    return Some(index);
                }
            }
        }
        None
    }

    pub fn can_allocate(&self, requested_dimensions: &DeviceIntSize) -> bool {
        self.find_index_of_best_rect(requested_dimensions).is_some()
    }

    pub fn allocate(
        &mut self,
        requested_dimensions: &DeviceIntSize
    ) -> Option<DeviceIntPoint> {
        if requested_dimensions.width == 0 || requested_dimensions.height == 0 {
            return Some(DeviceIntPoint::new(0, 0))
        }
        let index = match self.find_index_of_best_rect(requested_dimensions) {
            None => return None,
            Some(index) => index,
        };

        // Remove the rect from the free list and decide how to guillotine it. We choose the split
        // that results in the single largest area (Min Area Split Rule, MINAS).
        let chosen_rect = self.free_list.remove(index);
        let candidate_free_rect_to_right = DeviceIntRect {
            origin: DeviceIntPoint::new(chosen_rect.origin.x + requested_dimensions.width, chosen_rect.origin.y),
            size: DeviceIntSize::new(chosen_rect.size.width - requested_dimensions.width, requested_dimensions.height)
        };
        let candidate_free_rect_to_bottom =
            DeviceIntRect::new(
                DeviceIntPoint::new(chosen_rect.origin.x, chosen_rect.origin.y + requested_dimensions.height),
                DeviceIntSize::new(requested_dimensions.width, chosen_rect.size.height - requested_dimensions.height));
        let candidate_free_rect_to_right_area = candidate_free_rect_to_right.size.width *
            candidate_free_rect_to_right.size.height;
        let candidate_free_rect_to_bottom_area = candidate_free_rect_to_bottom.size.width *
            candidate_free_rect_to_bottom.size.height;

        // Guillotine the rectangle.
        let new_free_rect_to_right;
        let new_free_rect_to_bottom;
        if candidate_free_rect_to_right_area > candidate_free_rect_to_bottom_area {
            new_free_rect_to_right = DeviceIntRect::new(
                candidate_free_rect_to_right.origin,
                DeviceIntSize::new(candidate_free_rect_to_right.size.width,
                                    chosen_rect.size.height));
            new_free_rect_to_bottom = candidate_free_rect_to_bottom
        } else {
            new_free_rect_to_right = candidate_free_rect_to_right;
            new_free_rect_to_bottom =
                DeviceIntRect::new(candidate_free_rect_to_bottom.origin,
                          DeviceIntSize::new(chosen_rect.size.width,
                                              candidate_free_rect_to_bottom.size.height))
        }

        // Add the guillotined rects back to the free list. If any changes were made, we're now
        // dirty since coalescing might be able to defragment.
        if !rect_is_empty(&new_free_rect_to_right) {
            self.free_list.push(&new_free_rect_to_right);
            self.dirty = true
        }
        if !rect_is_empty(&new_free_rect_to_bottom) {
            self.free_list.push(&new_free_rect_to_bottom);
            self.dirty = true
        }

        // Bump the allocation counter.
        self.allocations += 1;

        // Return the result.
        Some(chosen_rect.origin)
    }

    fn coalesce_impl<F, U>(
        rects: &mut [DeviceIntRect],
        fun_key: F,
        fun_union: U
    ) -> CoalescingStatus where
        F: Fn(&DeviceIntRect) -> (i32, i32),
        U: Fn(&mut DeviceIntRect, &mut DeviceIntRect) -> usize,
    {
        let mut num_changed = 0;
        rects.sort_by_key(&fun_key);

        for work_index in 0..rects.len() {
            let (left, candidates) = rects.split_at_mut(work_index + 1);
            let item = left.last_mut().unwrap();
            if rect_is_empty(item) {
                continue
            }

            let key = fun_key(item);
            for candidate in candidates.iter_mut()
                                       .take_while(|r| key == fun_key(r)) {
                num_changed += fun_union(item, candidate);
            }
        }

        if num_changed > 0 {
            CoalescingStatus::Changed
        } else {
            CoalescingStatus::Unchanged
        }
    }

    /// Combine rects that have the same width and are adjacent.
    fn coalesce_horisontal(rects: &mut [DeviceIntRect]) -> CoalescingStatus {
        Self::coalesce_impl(rects,
                            |item| (item.size.width, item.origin.x),
                            |item, candidate| {
            if item.origin.y == candidate.max_y() || item.max_y() == candidate.origin.y {
                *item = item.union(candidate);
                candidate.size.width = 0;
                1
            } else { 0 }
        })
    }

    /// Combine rects that have the same height and are adjacent.
    fn coalesce_vertical(rects: &mut [DeviceIntRect]) -> CoalescingStatus {
        Self::coalesce_impl(rects,
                            |item| (item.size.height, item.origin.y),
                            |item, candidate| {
            if item.origin.x == candidate.max_x() || item.max_x() == candidate.origin.x {
                *item = item.union(candidate);
                candidate.size.height = 0;
                1
            } else { 0 }
        })
    }

    pub fn coalesce(&mut self) -> bool {
        if !self.dirty {
            return false
        }

        // Iterate to a fixed point.
        self.free_list.copy_to_vec(&mut self.coalesce_vec);
        let mut changed = false;

        //Note: we might want to consider try to use the last sorted order first
        // but the elements get shuffled around a bit anyway during the bin placement

        match Self::coalesce_horisontal(&mut self.coalesce_vec) {
            CoalescingStatus::Changed => changed = true,
            CoalescingStatus::Unchanged => (),
        }

        match Self::coalesce_vertical(&mut self.coalesce_vec) {
            CoalescingStatus::Changed => changed = true,
            CoalescingStatus::Unchanged => (),
        }

        if changed {
            self.free_list.init_from_slice(&self.coalesce_vec);
        }
        self.dirty = changed;
        changed
    }

    pub fn clear(&mut self) {
        self.free_list = FreeRectList::new();
        self.free_list.push(&DeviceIntRect::new(DeviceIntPoint::zero(), self.texture_size));
        self.allocations = 0;
        self.dirty = false;
    }

    pub fn free(&mut self, rect: &DeviceIntRect) {
        if rect_is_empty(rect) {
            return
        }
        debug_assert!(self.allocations > 0);
        self.allocations -= 1;
        if self.allocations == 0 {
            self.clear();
            return
        }

        self.free_list.push(rect);
        self.dirty = true
    }

    pub fn grow(&mut self, new_texture_size: DeviceIntSize) {
        assert!(new_texture_size.width >= self.texture_size.width);
        assert!(new_texture_size.height >= self.texture_size.height);

        let new_rects = [
            DeviceIntRect::new(DeviceIntPoint::new(self.texture_size.width, 0),
                                DeviceIntSize::new(new_texture_size.width - self.texture_size.width,
                                                    new_texture_size.height)),

            DeviceIntRect::new(DeviceIntPoint::new(0, self.texture_size.height),
                                DeviceIntSize::new(self.texture_size.width,
                                                    new_texture_size.height - self.texture_size.height)),
        ];

        for rect in &new_rects {
            if rect.size.width > 0 && rect.size.height > 0 {
                self.free_list.push(rect);
            }
        }

        self.texture_size = new_texture_size
    }

    pub fn can_grow(&self, max_size: i32) -> bool {
        self.texture_size.width < max_size || self.texture_size.height < max_size
    }
}

/// A binning free list. Binning is important to avoid sifting through lots of small strips when
/// allocating many texture items.
struct FreeRectList {
    small: Vec<DeviceIntRect>,
    medium: Vec<DeviceIntRect>,
    large: Vec<DeviceIntRect>,
}

impl FreeRectList {
    fn new() -> FreeRectList {
        FreeRectList {
            small: vec![],
            medium: vec![],
            large: vec![],
        }
    }

    fn init_from_slice(&mut self, rects: &[DeviceIntRect]) {
        self.small.clear();
        self.medium.clear();
        self.large.clear();
        for rect in rects {
            if !rect_is_empty(rect) {
                self.push(rect)
            }
        }
    }

    fn push(&mut self, rect: &DeviceIntRect) {
        match FreeListBin::for_size(&rect.size) {
            FreeListBin::Small => self.small.push(*rect),
            FreeListBin::Medium => self.medium.push(*rect),
            FreeListBin::Large => self.large.push(*rect),
        }
    }

    fn remove(&mut self, index: FreeListIndex) -> DeviceIntRect {
        match index.0 {
            FreeListBin::Small => self.small.swap_remove(index.1),
            FreeListBin::Medium => self.medium.swap_remove(index.1),
            FreeListBin::Large => self.large.swap_remove(index.1),
        }
    }

    fn iter(&self, bin: FreeListBin) -> Iter<DeviceIntRect> {
        match bin {
            FreeListBin::Small => self.small.iter(),
            FreeListBin::Medium => self.medium.iter(),
            FreeListBin::Large => self.large.iter(),
        }
    }

    fn copy_to_vec(&self, rects: &mut Vec<DeviceIntRect>) {
        rects.clear();
        rects.extend_from_slice(&self.small);
        rects.extend_from_slice(&self.medium);
        rects.extend_from_slice(&self.large);
    }
}

#[derive(Debug, Clone, Copy)]
struct FreeListIndex(FreeListBin, usize);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum FreeListBin {
    Small,
    Medium,
    Large,
}

impl FreeListBin {
    fn for_size(size: &DeviceIntSize) -> FreeListBin {
        if size.width >= MINIMUM_LARGE_RECT_SIZE && size.height >= MINIMUM_LARGE_RECT_SIZE {
            FreeListBin::Large
        } else if size.width >= MINIMUM_MEDIUM_RECT_SIZE &&
                size.height >= MINIMUM_MEDIUM_RECT_SIZE {
            FreeListBin::Medium
        } else {
            debug_assert!(size.width > 0 && size.height > 0);
            FreeListBin::Small
        }
    }
}



trait FitsInside {
    fn fits_inside(&self, other: &Self) -> bool;
}

impl FitsInside for DeviceIntSize {
    fn fits_inside(&self, other: &DeviceIntSize) -> bool {
        self.width <= other.width && self.height <= other.height
    }
}

