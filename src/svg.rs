use std::io::Write;
use crate::{FloatPoint, FloatRectangle, FloatSize};

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

