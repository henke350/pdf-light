//! pdf-light's own line icons, drawn in code (no icon font, no SVG library).
//!
//! Every icon is defined on a 16 × 16 grid (origin top-left, y down) in the
//! toolbar design spec. The data here is pure; only `paint` touches egui.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, pos2};

/// One icon per toolbar job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    PagesPanel,
    Open,
    Save,
    SaveAs,
    DeletePage,
    RotateLeft,
    RotateRight,
    Undo,
    Find,
    PreviousMatch,
    NextMatch,
    Export,
    Print,
    ZoomOut,
    ZoomIn,
    FitWidth,
}

impl Icon {
    /// Every icon, for the tests.
    #[cfg(test)]
    pub const ALL: [Icon; 16] = [
        Icon::PagesPanel,
        Icon::Open,
        Icon::Save,
        Icon::SaveAs,
        Icon::DeletePage,
        Icon::RotateLeft,
        Icon::RotateRight,
        Icon::Undo,
        Icon::Find,
        Icon::PreviousMatch,
        Icon::NextMatch,
        Icon::Export,
        Icon::Print,
        Icon::ZoomOut,
        Icon::ZoomIn,
        Icon::FitWidth,
    ];
}

/// One stroke of an icon, in grid units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Straight segments through `points`; `closed` joins the last point back to the first.
    // `&'static [..]` is a borrowed slice that lives for the whole program:
    // the points are constants baked into the exe, so nothing is allocated.
    Polyline {
        points: &'static [[f32; 2]],
        closed: bool,
    },
    Circle {
        centre: [f32; 2],
        radius: f32,
    },
}

/// An open polyline (a `const fn` can run at compile time, so the shapes stay constants).
const fn line(points: &'static [[f32; 2]]) -> Shape {
    Shape::Polyline {
        points,
        closed: false,
    }
}

/// A closed polyline: the last point joins back to the first.
const fn outline(points: &'static [[f32; 2]]) -> Shape {
    Shape::Polyline {
        points,
        closed: true,
    }
}

/// The shapes that make up `icon`, verbatim from the spec's icon table.
pub fn shapes(icon: Icon) -> &'static [Shape] {
    // A `match` on the enum hands back the data for each variant; the compiler
    // checks that every icon is covered. Each `const { .. }` block is worked out
    // at compile time, so its slice lives in the exe and can be `'static`.
    match icon {
        Icon::PagesPanel => {
            const {
                &[
                    outline(&[[1.5, 2.5], [14.5, 2.5], [14.5, 13.5], [1.5, 13.5]]),
                    line(&[[5.5, 2.5], [5.5, 13.5]]),
                ]
            }
        }
        Icon::Open => {
            const {
                &[outline(&[
                    [1.5, 4.0],
                    [6.0, 4.0],
                    [7.5, 5.5],
                    [14.5, 5.5],
                    [14.5, 13.0],
                    [1.5, 13.0],
                ])]
            }
        }
        Icon::Save => {
            const {
                &[
                    line(&[[8.0, 2.0], [8.0, 10.0]]),
                    line(&[[5.0, 7.0], [8.0, 10.0], [11.0, 7.0]]),
                    line(&[[2.0, 10.0], [2.0, 14.0], [14.0, 14.0], [14.0, 10.0]]),
                ]
            }
        }
        // The Save arrow and tray shifted right by 1.5, plus a small "+".
        Icon::SaveAs => {
            const {
                &[
                    line(&[[9.5, 2.0], [9.5, 10.0]]),
                    line(&[[6.5, 7.0], [9.5, 10.0], [12.5, 7.0]]),
                    line(&[[3.5, 10.0], [3.5, 14.0], [15.5, 14.0], [15.5, 10.0]]),
                    line(&[[1.5, 3.5], [5.5, 3.5]]),
                    line(&[[3.5, 1.5], [3.5, 5.5]]),
                ]
            }
        }
        Icon::DeletePage => {
            const {
                &[
                    outline(&[
                        [3.0, 1.5],
                        [10.0, 1.5],
                        [13.0, 4.5],
                        [13.0, 14.5],
                        [3.0, 14.5],
                    ]),
                    line(&[[10.0, 1.5], [10.0, 4.5], [13.0, 4.5]]),
                    line(&[[5.5, 9.5], [10.5, 9.5]]),
                ]
            }
        }
        // Three quarters of a circle with the arrowhead at its end, pointing
        // the way the page turns. Rotate left is Rotate right mirrored.
        Icon::RotateRight => {
            const {
                &[
                    line(&[
                        [3.17, 11.5],
                        [2.5, 9.0],
                        [3.17, 6.5],
                        [5.0, 4.67],
                        [7.5, 4.0],
                        [10.0, 4.67],
                        [11.83, 6.5],
                        [12.5, 9.0],
                    ]),
                    line(&[[10.0, 6.5], [12.5, 9.0], [15.0, 6.5]]),
                ]
            }
        }
        Icon::RotateLeft => {
            const {
                &[
                    line(&[
                        [12.83, 11.5],
                        [13.5, 9.0],
                        [12.83, 6.5],
                        [11.0, 4.67],
                        [8.5, 4.0],
                        [6.0, 4.67],
                        [4.17, 6.5],
                        [3.5, 9.0],
                    ]),
                    line(&[[6.0, 6.5], [3.5, 9.0], [1.0, 6.5]]),
                ]
            }
        }
        Icon::Undo => {
            const {
                &[
                    line(&[
                        [13.0, 12.0],
                        [13.0, 9.5],
                        [11.5, 7.0],
                        [8.5, 6.0],
                        [4.0, 6.0],
                    ]),
                    line(&[[6.5, 3.5], [4.0, 6.0], [6.5, 8.5]]),
                ]
            }
        }
        Icon::Find => {
            const {
                &[
                    Shape::Circle {
                        centre: [7.0, 7.0],
                        radius: 4.5,
                    },
                    line(&[[10.3, 10.3], [14.0, 14.0]]),
                ]
            }
        }
        Icon::PreviousMatch => const { &[line(&[[10.0, 3.0], [5.0, 8.0], [10.0, 13.0]])] },
        Icon::NextMatch => const { &[line(&[[6.0, 3.0], [11.0, 8.0], [6.0, 13.0]])] },
        Icon::Export => {
            const {
                &[
                    line(&[[9.0, 3.0], [2.0, 3.0], [2.0, 13.0], [9.0, 13.0]]),
                    line(&[[6.0, 8.0], [14.5, 8.0]]),
                    line(&[[11.5, 5.0], [14.5, 8.0], [11.5, 11.0]]),
                ]
            }
        }
        Icon::Print => {
            const {
                &[
                    outline(&[[2.0, 6.5], [14.0, 6.5], [14.0, 11.5], [2.0, 11.5]]),
                    line(&[[4.5, 6.5], [4.5, 2.0], [11.5, 2.0], [11.5, 6.5]]),
                    outline(&[[4.5, 9.5], [11.5, 9.5], [11.5, 14.5], [4.5, 14.5]]),
                ]
            }
        }
        Icon::ZoomOut => const { &[line(&[[3.0, 8.0], [13.0, 8.0]])] },
        Icon::ZoomIn => {
            const {
                &[
                    line(&[[3.0, 8.0], [13.0, 8.0]]),
                    line(&[[8.0, 3.0], [8.0, 13.0]]),
                ]
            }
        }
        Icon::FitWidth => {
            const {
                &[
                    line(&[[1.5, 3.0], [1.5, 13.0]]),
                    line(&[[14.5, 3.0], [14.5, 13.0]]),
                    line(&[[4.0, 8.0], [12.0, 8.0]]),
                    line(&[[6.0, 6.0], [4.0, 8.0], [6.0, 10.0]]),
                    line(&[[10.0, 6.0], [12.0, 8.0], [10.0, 10.0]]),
                ]
            }
        }
    }
}

/// Draws `icon` scaled from the 16-unit grid to `rect`, in `colour`, with a
/// 1.5-unit stroke and round caps and joins. `colour` should be opaque: the
/// round ends overlap the segments, which would show through a translucent colour.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, colour: Color32) {
    let unit = rect.width() / GRID;
    let to_screen =
        |[x, y]: [f32; 2]| -> Pos2 { pos2(rect.min.x + x * unit, rect.min.y + y * unit) };
    let stroke = Stroke::new(STROKE_UNITS * unit, colour);
    for shape in shapes(icon) {
        match *shape {
            Shape::Polyline { points, closed } => {
                // epaint has no round caps or joins, so each segment is drawn on
                // its own and every corner and end gets a dot as wide as the stroke.
                let screen: Vec<Pos2> = points.iter().copied().map(to_screen).collect();
                for pair in screen.windows(2) {
                    if let &[from, to] = pair {
                        painter.line_segment([from, to], stroke);
                    }
                }
                if closed && let (Some(&first), Some(&last)) = (screen.first(), screen.last()) {
                    painter.line_segment([last, first], stroke);
                }
                for &point in &screen {
                    painter.circle_filled(point, stroke.width / 2.0, colour);
                }
            }
            Shape::Circle { centre, radius } => {
                painter.circle_stroke(to_screen(centre), radius * unit, stroke);
            }
        }
    }
}

/// The grid is 16 units on each side.
const GRID: f32 = 16.0;
/// Stroke width in grid units (1.5 px at 16 px).
const STROKE_UNITS: f32 = 1.5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::toolbar::{ToolbarAction, icon_for};

    /// Every point an icon touches, with circles as their bounding-box corners.
    fn extents(shape: &Shape) -> Vec<[f32; 2]> {
        match *shape {
            Shape::Polyline { points, .. } => points.to_vec(),
            Shape::Circle { centre, radius } => vec![
                [centre[0] - radius, centre[1] - radius],
                [centre[0] + radius, centre[1] + radius],
            ],
        }
    }

    #[test]
    fn every_icon_should_stay_one_unit_inside_the_grid() {
        let outside: Vec<(Icon, [f32; 2])> = Icon::ALL
            .iter()
            .flat_map(|&icon| {
                shapes(icon)
                    .iter()
                    .flat_map(extents)
                    .map(move |point| (icon, point))
            })
            .filter(|(_, [x, y])| !(0.5..=15.5).contains(x) || !(0.5..=15.5).contains(y))
            .collect();
        assert_eq!(outside, vec![]);
    }

    #[test]
    fn every_icon_should_have_at_least_one_shape() {
        let empty: Vec<Icon> = Icon::ALL
            .into_iter()
            .filter(|&icon| shapes(icon).is_empty())
            .collect();
        assert_eq!(empty, vec![]);
    }

    /// Fails to compile when `ToolbarAction` gains a variant, as a reminder
    /// to add it to the list in `toolbar_buttons_should_all_have_icons`.
    fn _listed(action: ToolbarAction) {
        match action {
            ToolbarAction::TogglePagesPanel
            | ToolbarAction::Open
            | ToolbarAction::Save
            | ToolbarAction::SaveAs
            | ToolbarAction::DeleteSelected
            | ToolbarAction::RotateLeft
            | ToolbarAction::RotateRight
            | ToolbarAction::Undo
            | ToolbarAction::FindNext
            | ToolbarAction::FindPrevious
            | ToolbarAction::Export
            | ToolbarAction::Print
            | ToolbarAction::ZoomIn
            | ToolbarAction::ZoomOut
            | ToolbarAction::SetZoom(_)
            | ToolbarAction::FitWidth => {}
        }
    }

    #[test]
    fn toolbar_buttons_should_all_have_icons() {
        let actions = [
            ToolbarAction::TogglePagesPanel,
            ToolbarAction::Open,
            ToolbarAction::Save,
            ToolbarAction::SaveAs,
            ToolbarAction::DeleteSelected,
            ToolbarAction::RotateLeft,
            ToolbarAction::RotateRight,
            ToolbarAction::Undo,
            ToolbarAction::FindNext,
            ToolbarAction::FindPrevious,
            ToolbarAction::Export,
            ToolbarAction::Print,
            ToolbarAction::ZoomIn,
            ToolbarAction::ZoomOut,
            ToolbarAction::FitWidth,
        ];
        let missing: Vec<ToolbarAction> = actions
            .into_iter()
            .filter(|&action| icon_for(action).is_none())
            .collect();
        assert_eq!(missing, vec![]);
    }
}
