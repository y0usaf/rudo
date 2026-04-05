use itertools::Itertools;
use skia_safe::{
    BlendMode, Canvas, Color, Paint, Path, PathBuilder, PathOp, Rect,
    canvas::SaveLayerRec,
};

use glamour::Intersection;

use crate::units::{GridScale, PixelRect, to_skia_rect};

use super::{RenderedWindow, WindowDrawDetails, is_rightmost_window_edge};

struct LayerWindow<'w> {
    window: &'w mut RenderedWindow,
    group: usize,
}

pub struct FloatingLayer<'w> {
    pub windows: Vec<&'w mut RenderedWindow>,
}

impl FloatingLayer<'_> {
    fn build_draw_clip_and_bounds(
        &self,
        mut draw_clip: Path,
        mut draw_bound_rect: Rect,
        expanded_regions: &[PixelRect<f32>],
        grid_scale: GridScale,
    ) -> (Path, Rect) {
        for (window, region) in self.windows.iter().zip(expanded_regions.iter().copied()) {
            if let Some((path, bounds)) = window.trailing_fill_path_and_bounds(region, grid_scale) {
                if let Some(unioned) = draw_clip.op(&path, PathOp::Union) {
                    draw_clip = unioned;
                }
                draw_bound_rect = Rect::join2(draw_bound_rect, bounds);
            }
        }

        (draw_clip, draw_bound_rect)
    }

    pub fn draw(
        &mut self,
        root_canvas: &Canvas,
        default_background: Color,
        grid_scale: GridScale,
        content_region: Option<PixelRect<f32>>,
    ) -> Vec<WindowDrawDetails> {
        let pixel_regions =
            self.windows.iter().map(|window| window.pixel_region(grid_scale)).collect::<Vec<_>>();
        let max_layer_x = max_region_max_x(&pixel_regions);
        let regions = self
            .windows
            .iter()
            .zip(pixel_regions.iter().copied())
            .map(|(window, region)| {
                let rightmost_window = is_rightmost_window_edge(region.max.x, max_layer_x);
                window.expanded_pixel_region(region, content_region, grid_scale, rightmost_window)
            })
            .collect::<Vec<_>>();

        let silhouette = build_silhouette(&pixel_regions, grid_scale);
        let (draw_clip, draw_bound_rect) =
            self.build_draw_clip_and_bounds(silhouette.clone(), Rect::default(), &regions, grid_scale);

        root_canvas.save();
        root_canvas.clip_path(&draw_clip, None, Some(false));

        let paint =
            Paint::default().set_anti_alias(false).set_blend_mode(BlendMode::SrcOver).to_owned();

        let save_layer_rec = SaveLayerRec::default().bounds(&draw_bound_rect).paint(&paint);

        root_canvas.save_layer(&save_layer_rec);
        let background_paint = Paint::default().set_color(default_background).to_owned();
        root_canvas.draw_path(&draw_clip, &background_paint);
        let mut ret = vec![];

        (0..self.windows.len()).for_each(|i| {
            let window = &mut self.windows[i];
            window.draw_background_surface(root_canvas, pixel_regions[i], grid_scale);
            window.draw_foreground_surface(root_canvas, pixel_regions[i], grid_scale);
            ret.push(WindowDrawDetails {
                id: window.id,
                region: regions[i],
                grid_size: window.grid_size,
            });
        });

        for (window, region) in self.windows.iter().zip(regions.iter().copied()) {
            window.draw_trailing_background_surface(root_canvas, region, grid_scale);
        }

        root_canvas.restore();

        root_canvas.restore();

        ret
    }

}

fn get_window_group(windows: &mut Vec<LayerWindow>, index: usize) -> usize {
    if windows[index].group != index {
        windows[index].group = get_window_group(windows, windows[index].group);
    }
    windows[index].group
}

fn group_windows_with_regions(windows: &mut Vec<LayerWindow>, regions: &[PixelRect<f32>]) {
    // intersects does not consider touching regions as intersection, so extend the box by one
    // pixel before doing the test.
    let epsilon = 1.0;
    for i in 0..windows.len() {
        for j in i + 1..windows.len() {
            let group_i = get_window_group(windows, i);
            let group_j = get_window_group(windows, j);
            if group_i != group_j
                && regions[i].to_rect().inflate((epsilon, epsilon).into()).intersects(&regions[j])
            {
                let new_group = group_i.min(group_j);
                if group_i != group_j {
                    windows[group_i].group = new_group;
                    windows[group_j].group = new_group;
                }
            }
        }
    }
}

pub fn group_windows(
    windows: Vec<&mut RenderedWindow>,
    grid_scale: GridScale,
) -> Vec<Vec<&mut RenderedWindow>> {
    let mut windows = windows
        .into_iter()
        .enumerate()
        .map(|(index, window)| LayerWindow { window, group: index })
        .collect::<Vec<_>>();
    let regions =
        windows.iter().map(|window| window.window.pixel_region(grid_scale)).collect::<Vec<_>>();
    group_windows_with_regions(&mut windows, &regions);
    for i in 0..windows.len() {
        let _ = get_window_group(&mut windows, i);
    }
    windows.sort_by(|a, b| a.group.cmp(&b.group));
    windows
        .into_iter()
        .chunk_by(|window| window.group)
        .into_iter()
        .map(|(_, v)| v.map(|w| w.window).collect::<Vec<_>>())
        .collect_vec()
}

fn build_silhouette(
    regions: &[PixelRect<f32>],
    _grid_scale: GridScale,
) -> Path {
    regions
        .iter()
        .map(|r| {
            let rect = to_skia_rect(r);
            let mut builder = PathBuilder::new();
            builder.add_rect(rect, None, None);
            builder.detach()
        })
        .reduce(|a, b| a.op(&b, PathOp::Union).unwrap())
        .unwrap()
}

fn max_region_max_x(regions: &[PixelRect<f32>]) -> f32 {
    regions.iter().fold(f32::NEG_INFINITY, |max_x, region| max_x.max(region.max.x))
}


