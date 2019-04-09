use std::io::Write;
use euclid::{point2, vec2, size2};
use crate::{FloatPoint, FloatRectangle, FloatSize};
use crate::{GuillotineAllocator, BuiltGraph};

pub fn rectangle(output: &mut dyn Write, rect: &FloatRectangle, radius: f32, style: &str) {
    write!(output,
        r#"    <rect ry="{}" x="{}" y="{}" width="{}" height="{}" style="{}" />"#,
        radius,
        rect.min.x,
        rect.min.y,
        rect.size().width,
        rect.size().height,
        style
    ).unwrap();
}

pub fn text(output: &mut dyn Write, text: &str, size: f32, position: FloatPoint, style: &str) {
    write!(output,
r#"
    <text x="{}" y="{}" style="{};font-style:normal;font-weight:normal;font-size:{}px;line-height:1.25;font-family:sans-serif;letter-spacing:0px;word-spacing:0px;fill:#000000;fill-opacity:1;stroke:none;stroke-width:0.26458332" xml:space="preserve">
        <tspan>{}</tspan>
    </text>
"#,
        position.x, position.y,
        style,
        size,
        text,
    ).unwrap();
}

pub fn begin_svg(output: &mut dyn Write, size: &FloatSize) {
    write!(output,
r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<svg
   xmlns:cc="http://creativecommons.org/ns#"
   xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
   xmlns:svg="http://www.w3.org/2000/svg"
   xmlns="http://www.w3.org/2000/svg"
   version="1.1"
   viewBox="0 0 {} {}"
   width="{}mm"
   height="{}mm">
"#,
        size.width,
        size.height,
        size.width,
        size.height,
    ).unwrap();
}

pub fn end_svg(output: &mut dyn Write) {
    write!(output, "</svg>").unwrap();
}

pub fn link(output: &mut Write, from: FloatPoint, to: FloatPoint, style: &str) {
    let mid_x = (from.x + to.x) * 0.5 ;

    write!(output,
r#"
    <path d="M {} {} C {} {} {} {} {} {}" style="fill:none;{}" />
"#,
        from.x, from.y,
        mid_x, from.y,
        mid_x, to.y,
        to.x, to.y,
        style,
    ).unwrap();
}

#[derive(Copy, Clone, Debug)]
pub struct VerticalLayout {
    pub start: FloatPoint,
    pub y: f32,
    pub width: f32,
}

impl VerticalLayout {
    fn new(start: FloatPoint, width: f32) -> Self {
        VerticalLayout {
            start,
            y: start.y,
            width,
        }
    }

    fn advance(&mut self, by: f32) {
        self.y += by;
    }

    fn push_rectangle(&mut self, height: f32) -> FloatRectangle {
        let rect = FloatRectangle {
            min: point2(self.start.x, self.y),
            max: point2(self.start.x + self.width, self.y + height),
        };
        self.y += height;

        rect
    }

    fn total_rectangle(&self) -> FloatRectangle {
        FloatRectangle {
            min: self.start,
            max: point2(self.start.x + self.width, self.y),
        }
    }

    fn start_here(&mut self) {
        self.start.y = self.y;
    }
}

pub fn dump_svg(output: &mut dyn std::io::Write, graph: &BuiltGraph, allocator: &GuillotineAllocator) {
    let node_width = 80.0;
    let node_height = 40.0;
    let texture_box_height = 15.0;
    let vertical_spacing = 10.0;
    let horizontal_spacing = 40.0;
    let margin = 10.0;

    let mut target_rects = Vec::new();
    let mut texture_info = Vec::new();
    let mut node_label_rects = vec![None; graph.nodes.len()];
    let mut x = margin;
    let mut max_y: f32 = 0.0;
    for pass in &graph.passes {
        let mut layout = VerticalLayout::new(point2(x, margin), node_width);
        for target in &pass.targets {
            if target.nodes.is_empty() {
                continue;
            }

            layout.start_here();
            let mut allocated_rects = Vec::new();
            for &node in &target.nodes {
                node_label_rects[node.to_usize()] = Some(layout.push_rectangle(node_height));
                layout.advance(vertical_spacing);
                allocated_rects.push(graph.allocated_rects[node.to_usize()].unwrap());
            }

            let texture_label_rect = layout.push_rectangle(texture_box_height);

            let tex_size = allocator.textures[target.destination.unwrap().to_usize()].size().to_f32();
            let scale = tex_size.width / node_width;
            layout.push_rectangle(tex_size.height / scale);

            target_rects.push(layout.total_rectangle().inflate(5.0, 5.0));

            texture_info.push((
                texture_label_rect,
                target.destination,
                allocated_rects,
            ));
        }
        x += node_width + horizontal_spacing;
        max_y = max_y.max(layout.y + 100.0);
    }

    let svg_size: FloatSize = size2(x + margin, max_y + margin);
    begin_svg(output, &svg_size);
    let bg_rect = FloatRectangle {
        min: point2(0.0, 0.0),
        max: point2(svg_size.width, svg_size.height),
    }.inflate(1.0, 1.0);
    rectangle(output, &bg_rect, 0.0, "fill:rgb(50,50,50)");

    for rect in &target_rects {
        rectangle(output, rect, 5.0, "stroke:none;fill:black;fill-opacity:0.2");
    }

    for (i, rect) in node_label_rects.iter().enumerate() {
        if let Some(rect) = rect {
            let pos = rect.min;
            for input in &graph.nodes[i].dependencies {
                let input_pos = node_label_rects[input.to_usize()].unwrap().min;
                let from = input_pos + vec2(node_width, node_height / 2.0);
                let to = pos + vec2(0.0, node_height / 2.0);
                link(output, from + vec2(0.0, 1.0), to + vec2(0.0, 1.0), "stroke:black;stroke-opacity:0.4;stroke-width:3px;");
                link(output, from, to, "stroke:rgb(100, 100, 100);stroke-width:3px;");
            }
        }
    }

    for rect in &node_label_rects {
        if let Some(rect) = rect {
            rectangle(output, &rect.translate(&vec2(0.0, 2.0)), 3.0, "stroke:none;fill:black;fill-opacity:0.4");
            rectangle(output, rect, 3.0, "stroke:none;fill:rgb(200, 200, 200);fill-opacity:0.8");
        }
    }

    for &(ref rect, dest, ref alloc_rects) in &texture_info {
        let atlas_min = rect.min + vec2(0.0, texture_box_height);
        let tex_size = allocator.textures[dest.unwrap().to_usize()].size().to_f32();
        let scale = tex_size.width / node_width;
        let atlas_rect = FloatRectangle {
            min: atlas_min,
            max: atlas_min + vec2(tex_size.width, tex_size.height) / scale,
        };

        // Per-texture label.
        rectangle(output, &rect.translate(&vec2(0.0, 2.0)), 3.0, "stroke:none;fill:black;fill-opacity:0.4");
        rectangle(output, rect, 3.0, "stroke:none;fill:rgb(200, 200, 200);fill-opacity:0.8");
        let text_pos = point2((rect.min.x + rect.max.x)/2.0 - 5.0, rect.min.y + 10.0);
        text(output, &format!("{:?} {}", dest.unwrap(), tex_size), 6.0, text_pos, "text-anchor:middle;text-align:center;");

        // Atlas.
        rectangle(output, &atlas_rect, 0.0, "stroke:none;fill:black;fill-opacity:0.5");
        for alloc in alloc_rects {
            let scaled_rect = alloc.rectangle.to_f32() / scale;
            rectangle(output, &scaled_rect.translate(&atlas_rect.min.to_vector()), 0.0, "stroke:none;fill:rgb(200, 0, 0);fill-opacity:0.8")
        }
    }

    for (i, rect) in node_label_rects.iter().enumerate() {
        if let Some(rect) = rect {
            let pos = point2((rect.min.x + rect.max.x)/2.0 - 6.0, (rect.min.y + rect.max.y) / 2.0);
            text(output, &graph.nodes[i].name, 10.0, pos, "text-anchor:middle;text-align:center;");
        }
    }

    end_svg(output);
}

