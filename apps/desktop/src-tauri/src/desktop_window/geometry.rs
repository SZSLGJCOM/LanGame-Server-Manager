//! Physical window sizing and content density relative to the reference display.

use tauri::{LogicalSize, PhysicalPosition, PhysicalSize};

const WINDOWED_WORK_AREA_RATIO: f64 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DisplayArea {
    pub(super) origin: PhysicalPosition<i32>,
    pub(super) size: PhysicalSize<u32>,
    pub(super) screen: PhysicalSize<u32>,
    pub(super) scale: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Geometry {
    pub(super) position: PhysicalPosition<i32>,
    pub(super) inner: PhysicalSize<u32>,
    pub(super) outer: PhysicalSize<u32>,
}

#[derive(Debug, PartialEq)]
pub(super) struct Placement {
    pub(super) inner: PhysicalSize<u32>,
    pub(super) position: PhysicalPosition<i32>,
}

fn screen_scale(display: DisplayArea) -> f64 {
    (f64::from(display.screen.width) / 2560.0).min(f64::from(display.screen.height) / 1440.0)
}

pub(super) fn zoom_factor(display: DisplayArea) -> f64 {
    // Keep the reference canvas density from 1080p upward. Below that, let the
    // responsive layout use fewer CSS pixels instead of shrinking text forever.
    screen_scale(display).max(0.75) / display.scale
}

pub(super) fn placement(
    display: DisplayArea,
    preferred: LogicalSize<f64>,
    current: Geometry,
    center: bool,
) -> Placement {
    // The configured canvas is the physical size on the 2560 x 1440 reference
    // display. Windows DPI must not enlarge it. Keep the same aspect ratio and
    // leave desktop space; the work-area cap includes native frame extents.
    let frame = PhysicalSize::new(
        current.outer.width.saturating_sub(current.inner.width),
        current.outer.height.saturating_sub(current.inner.height),
    );
    let available = PhysicalSize::new(
        ((f64::from(display.size.width) * WINDOWED_WORK_AREA_RATIO) as u32)
            .saturating_sub(frame.width)
            .max(1),
        ((f64::from(display.size.height) * WINDOWED_WORK_AREA_RATIO) as u32)
            .saturating_sub(frame.height)
            .max(1),
    );
    let scale = screen_scale(display)
        .min(f64::from(available.width) / preferred.width)
        .min(f64::from(available.height) / preferred.height);
    let inner = PhysicalSize::new(
        ((preferred.width * scale).round() as u32).clamp(1, available.width),
        ((preferred.height * scale).round() as u32).clamp(1, available.height),
    );
    let outer = PhysicalSize::new(
        inner.width.saturating_add(frame.width),
        inner.height.saturating_add(frame.height),
    );
    let coordinate = |origin: i32, extent: u32, size: u32, current: i32| {
        let start = i64::from(origin);
        let remaining = i64::from(extent.saturating_sub(size));
        let position = if center {
            start + remaining / 2
        } else {
            i64::from(current).clamp(start, start + remaining)
        };
        position.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    Placement {
        inner,
        position: PhysicalPosition::new(
            coordinate(
                display.origin.x,
                display.size.width,
                outer.width,
                current.position.x,
            ),
            coordinate(
                display.origin.y,
                display.size.height,
                outer.height,
                current.position.y,
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferred() -> LogicalSize<f64> {
        LogicalSize::new(1560.0, 900.0)
    }

    fn display(width: u32, height: u32, work_height: u32, scale: f64) -> DisplayArea {
        DisplayArea {
            origin: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(width, work_height),
            screen: PhysicalSize::new(width, height),
            scale,
        }
    }

    fn window(width: u32, height: u32) -> Geometry {
        Geometry {
            position: PhysicalPosition::new(0, 0),
            inner: PhysicalSize::new(width, height),
            outer: PhysicalSize::new(width, height),
        }
    }

    #[test]
    fn reference_desktop_keeps_the_existing_comfortable_window() {
        let area = display(2560, 1440, 1392, 1.0);
        let fit = placement(area, preferred(), window(1560, 900), true);
        assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        assert_eq!(fit.position, PhysicalPosition::new(500, 246));
        assert_eq!(zoom_factor(area), 1.0);
    }

    #[test]
    fn common_resolutions_keep_the_canvas_and_ignore_system_dpi_for_physical_size() {
        for (width, height, work_height, physical_width, physical_height, content_scale) in [
            (1920, 1080, 1040, 1170, 675, 0.75),
            (2560, 1440, 1392, 1560, 900, 1.0),
            (3840, 2160, 2080, 2340, 1350, 1.5),
        ] {
            let mut initial = None;
            for dpi in [1.0, 1.25, 1.5, 2.0] {
                let area = display(width, height, work_height, dpi);
                let fit = placement(area, preferred(), window(900, 600), true);
                assert_eq!(
                    fit.inner,
                    PhysicalSize::new(physical_width, physical_height)
                );
                assert!((dpi * zoom_factor(area) - content_scale).abs() < 1e-10);
                let css = fit.inner.to_logical::<f64>(dpi * zoom_factor(area));
                assert!((css.width - 1560.0).abs() < 1e-10);
                assert!((css.height - 900.0).abs() < 1e-10);
                if let Some(initial) = &initial {
                    assert_eq!(&fit, initial);
                } else {
                    initial = Some(fit);
                }
            }
        }
    }

    #[test]
    fn small_displays_stop_shrinking_text_and_use_a_smaller_css_viewport() {
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            let area = display(1280, 720, 680, dpi);
            let fit = placement(area, preferred(), window(1560, 900), true);
            assert_eq!(fit.inner, PhysicalSize::new(780, 450));
            assert!((dpi * zoom_factor(area) - 0.75).abs() < 1e-10);
            assert_eq!(
                fit.inner.to_logical::<f64>(dpi * zoom_factor(area)),
                LogicalSize::new(1040.0, 600.0)
            );
        }
    }

    #[test]
    fn work_area_cap_accounts_for_the_native_frame_without_changing_text_scale() {
        let mut area = display(2560, 1440, 1392, 1.5);
        area.origin = PhysicalPosition::new(40, 80);
        area.size = PhysicalSize::new(1280, 720);
        let mut current = window(1560, 900);
        current.outer = PhysicalSize::new(1576, 939);
        let fit = placement(area, preferred(), current, true);
        assert_eq!(fit.inner, PhysicalSize::new(1056, 609));
        assert_eq!(fit.position, PhysicalPosition::new(144, 116));
        assert_eq!(zoom_factor(area), 1.0 / 1.5);
    }

    #[test]
    fn taskbar_changes_do_not_change_the_screen_reference_or_content_density() {
        let normal = display(2560, 1440, 1392, 1.5);
        let taller_taskbar = display(2560, 1440, 1300, 1.5);
        for area in [normal, taller_taskbar] {
            assert_eq!(
                placement(area, preferred(), window(900, 600), true).inner,
                PhysicalSize::new(1560, 900)
            );
            assert_eq!(zoom_factor(area), 1.0 / 1.5);
        }
    }

    #[test]
    fn moving_between_monitors_updates_size_and_density_and_clamps_negative_coordinates() {
        let mut secondary = display(1920, 1080, 1040, 1.5);
        secondary.origin = PhysicalPosition::new(-1920, 40);
        let mut current = window(1560, 900);
        current.position = PhysicalPosition::new(-400, -200);
        let fit = placement(secondary, preferred(), current, false);
        assert_eq!(fit.inner, PhysicalSize::new(1170, 675));
        assert_eq!(fit.position, PhysicalPosition::new(-1170, 40));
        assert_eq!(zoom_factor(secondary), 0.5);
        let larger = display(3840, 2160, 2080, 2.0);
        current.position = PhysicalPosition::new(100, 80);
        let restored = placement(larger, preferred(), current, false);
        assert_eq!(restored.inner, PhysicalSize::new(2340, 1350));
        assert_eq!(restored.position, current.position);
        assert_eq!(zoom_factor(larger), 0.75);
    }

    #[test]
    fn arbitrary_resize_and_snap_sizes_cannot_change_the_windowed_shape() {
        for size in [(700, 900), (2560, 1392), (1280, 1392), (1200, 700)] {
            let fit = placement(
                display(2560, 1440, 1392, 1.5),
                preferred(),
                window(size.0, size.1),
                false,
            );
            assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        }
    }

    #[test]
    fn fitted_window_keeps_its_ratio_and_leaves_space_in_portrait_and_landscape() {
        for (width, height, work_height) in [
            (1280, 720, 680),
            (1366, 768, 728),
            (1920, 1080, 1040),
            (2560, 1440, 1392),
            (1080, 1920, 1880),
            (3440, 1440, 1400),
            (3840, 2160, 2080),
        ] {
            for dpi in [1.0, 1.25, 1.5, 2.0] {
                for (frame_width, frame_height) in [(0, 0), (16, 39)] {
                    let mut current = window(1560, 900);
                    current.outer = PhysicalSize::new(1560 + frame_width, 900 + frame_height);
                    let fit = placement(
                        display(width, height, work_height, dpi),
                        preferred(),
                        current,
                        true,
                    );
                    let outer = PhysicalSize::new(
                        fit.inner.width + frame_width,
                        fit.inner.height + frame_height,
                    );
                    assert!(fit.position.x >= 0 && fit.position.y >= 0);
                    assert!(fit.position.x as u32 + outer.width <= width);
                    assert!(fit.position.y as u32 + outer.height <= work_height);
                    assert!(u64::from(outer.width) * 10 <= u64::from(width) * 9);
                    assert!(u64::from(outer.height) * 10 <= u64::from(work_height) * 9);
                    assert!(
                        (i64::from(fit.position.x) * 2 + i64::from(outer.width) - i64::from(width))
                            .abs()
                            <= 1
                    );
                    assert!(
                        (i64::from(fit.position.y) * 2 + i64::from(outer.height)
                            - i64::from(work_height))
                        .abs()
                            <= 1
                    );
                    let ratio_error = (f64::from(fit.inner.width)
                        - f64::from(fit.inner.height) * 1560.0 / 900.0)
                        .abs();
                    assert!(
                        ratio_error <= 1.0,
                        "aspect ratio rounding exceeded one pixel: {fit:?}"
                    );
                }
            }
        }
    }
}
