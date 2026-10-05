//! Procedurally generated printed-circuit-board background with signal pulses.

use super::theme::{alpha, BG, GOLD, GREEN};
use eframe::egui::{epaint::Mesh, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};

const CELL: f32 = 22.0;
const COLS: i32 = 180;
const ROWS: i32 = 110;

const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % ((hi - lo).max(1) as u64)) as i32
    }
}

struct Trace {
    points: Vec<Pos2>,
    seg_len: Vec<f32>,
    total: f32,
    bbox: Rect,
    gold: bool,
    start_via: bool,
    pulse: Option<(f32, f32)>,
}

struct Chip {
    rect: Rect,
    label: String,
    pins: Vec<[Pos2; 2]>,
}

pub struct Circuit {
    traces: Vec<Trace>,
    chips: Vec<Chip>,
}

struct Grid {
    occ: Vec<bool>,
}

impl Grid {
    fn inside(x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < COLS && y < ROWS
    }

    fn get(&self, x: i32, y: i32) -> bool {
        !Self::inside(x, y) || self.occ[(y * COLS + x) as usize]
    }

    fn set(&mut self, x: i32, y: i32) {
        if Self::inside(x, y) {
            self.occ[(y * COLS + x) as usize] = true;
        }
    }
}

fn cell_center(x: i32, y: i32) -> Pos2 {
    Pos2::new(x as f32 * CELL + CELL / 2.0, y as f32 * CELL + CELL / 2.0)
}

fn walk(grid: &mut Grid, rng: &mut Rng, start: (i32, i32), mut dir: usize, max_len: i32) -> Vec<(i32, i32)> {
    let mut cells = vec![start];
    grid.set(start.0, start.1);
    let mut cur = start;
    for step in 0..max_len {
        if step > 1 && rng.f() < 0.2 {
            dir = (dir + if rng.f() < 0.5 { 1 } else { 7 }) % 8;
        }
        let mut moved = false;
        for attempt in [0usize, 1, 7] {
            let d = (dir + attempt) % 8;
            let (dx, dy) = DIRS[d];
            let next = (cur.0 + dx, cur.1 + dy);
            if grid.get(next.0, next.1) {
                continue;
            }
            // Do not cut diagonally between two occupied cells.
            if dx != 0 && dy != 0 && grid.get(cur.0 + dx, cur.1) && grid.get(cur.0, cur.1 + dy) {
                continue;
            }
            dir = d;
            cur = next;
            grid.set(cur.0, cur.1);
            cells.push(cur);
            moved = true;
            break;
        }
        if !moved {
            break;
        }
    }
    cells
}

/// Keeps only the corner points of a cell path.
fn corners(cells: &[(i32, i32)]) -> Vec<Pos2> {
    let mut pts = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        if i == 0 || i == cells.len() - 1 {
            pts.push(cell_center(c.0, c.1));
            continue;
        }
        let prev = cells[i - 1];
        let next = cells[i + 1];
        let d1 = (c.0 - prev.0, c.1 - prev.1);
        let d2 = (next.0 - c.0, next.1 - c.1);
        if d1 != d2 {
            pts.push(cell_center(c.0, c.1));
        }
    }
    pts
}

fn make_trace(points: Vec<Pos2>, gold: bool, start_via: bool, rng: &mut Rng) -> Trace {
    let mut seg_len = Vec::with_capacity(points.len());
    let mut total = 0.0;
    for w in points.windows(2) {
        let l = (w[1] - w[0]).length();
        seg_len.push(l);
        total += l;
    }
    let bbox = Rect::from_points(&points).expand(6.0);
    let pulse = if total > 140.0 && rng.f() < 0.22 {
        Some((55.0 + rng.f() * 110.0, rng.f() * 2000.0))
    } else {
        None
    };
    Trace { points, seg_len, total, bbox, gold, start_via, pulse }
}

impl Circuit {
    pub fn generate(seed: u64) -> Circuit {
        let mut rng = Rng(seed | 1);
        let mut grid = Grid { occ: vec![false; (COLS * ROWS) as usize] };
        let mut traces = Vec::new();
        let mut chips = Vec::new();

        // Chips first, so traces route around them.
        let chip_count = 26;
        let mut attempts = 0;
        while chips.len() < chip_count && attempts < 600 {
            attempts += 1;
            let w = rng.range(4, 9);
            let h = rng.range(3, 6);
            let x = rng.range(2, COLS - w - 2);
            let y = rng.range(2, ROWS - h - 2);
            let mut free = true;
            'check: for yy in (y - 2)..(y + h + 2) {
                for xx in (x - 2)..(x + w + 2) {
                    if grid.get(xx, yy) {
                        free = false;
                        break 'check;
                    }
                }
            }
            if !free {
                continue;
            }
            for yy in y..(y + h) {
                for xx in x..(x + w) {
                    grid.set(xx, yy);
                }
            }
            let rect = Rect::from_min_max(
                Pos2::new(x as f32 * CELL + 3.0, y as f32 * CELL + 3.0),
                Pos2::new((x + w) as f32 * CELL - 3.0, (y + h) as f32 * CELL - 3.0),
            );
            let mut pins = Vec::new();
            let mut starts: Vec<((i32, i32), usize)> = Vec::new();
            for xx in x..(x + w) {
                let cx = xx as f32 * CELL + CELL / 2.0;
                pins.push([Pos2::new(cx, rect.top()), Pos2::new(cx, rect.top() - 6.0)]);
                pins.push([Pos2::new(cx, rect.bottom()), Pos2::new(cx, rect.bottom() + 6.0)]);
                starts.push(((xx, y - 1), 6));
                starts.push(((xx, y + h), 2));
            }
            for yy in y..(y + h) {
                let cy = yy as f32 * CELL + CELL / 2.0;
                pins.push([Pos2::new(rect.left(), cy), Pos2::new(rect.left() - 6.0, cy)]);
                pins.push([Pos2::new(rect.right(), cy), Pos2::new(rect.right() + 6.0, cy)]);
                starts.push(((x - 1, yy), 4));
                starts.push(((x + w, yy), 0));
            }
            let label = format!("FC-{:04X}", rng.next() as u16);
            chips.push(Chip { rect, label, pins });

            for (cell, dir) in starts {
                if rng.f() < 0.55 && !grid.get(cell.0, cell.1) {
                    let len = rng.range(4, 22);
                    let cells = walk(&mut grid, &mut rng, cell, dir, len);
                    if cells.len() >= 3 {
                        let gold = rng.f() < 0.12;
                        traces.push(make_trace(corners(&cells), gold, false, &mut rng));
                    }
                }
            }
        }

        // Free-running traces fill the rest of the board.
        let count = (COLS * ROWS / 20) as usize;
        for _ in 0..count {
            let x = rng.range(0, COLS);
            let y = rng.range(0, ROWS);
            if grid.get(x, y) {
                continue;
            }
            let dir = if rng.f() < 0.75 { [0usize, 2, 4, 6][rng.range(0, 4) as usize] } else { rng.range(0, 8) as usize };
            let len = rng.range(3, 18);
            let cells = walk(&mut grid, &mut rng, (x, y), dir, len);
            if cells.len() >= 3 {
                let gold = rng.f() < 0.1;
                traces.push(make_trace(corners(&cells), gold, true, &mut rng));
            }
        }
        Circuit { traces, chips }
    }

    fn point_at(t: &Trace, mut d: f32) -> Pos2 {
        for (i, l) in t.seg_len.iter().enumerate() {
            if d <= *l {
                let a = t.points[i];
                let b = t.points[i + 1];
                return a + (b - a) * (d / l.max(0.001));
            }
            d -= l;
        }
        *t.points.last().unwrap_or(&Pos2::ZERO)
    }

    pub fn paint(&self, painter: &Painter, screen: Rect, time: f32, animate: bool, intensity: f32) {
        painter.rect_filled(screen, 0.0, BG);

        // Fine grid dots
        let dot = alpha(GREEN, 10);
        let mut y = (CELL * 0.5).max(screen.top());
        while y < screen.bottom() {
            let mut x = CELL * 0.5;
            while x < screen.right() {
                painter.circle_filled(Pos2::new(x, y), 0.8, dot);
                x += CELL * 2.0;
            }
            y += CELL * 2.0;
        }

        let green_line = alpha(GREEN, (26.0 * intensity) as u8);
        let gold_line = alpha(GOLD, (34.0 * intensity) as u8);
        for t in &self.traces {
            if !t.bbox.intersects(screen) {
                continue;
            }
            let color = if t.gold { gold_line } else { green_line };
            painter.add(Shape::line(t.points.clone(), Stroke::new(1.6, color)));
            let via_color = if t.gold { alpha(GOLD, 70) } else { alpha(GREEN, 60) };
            let mut ends = vec![*t.points.last().unwrap_or(&Pos2::ZERO)];
            if t.start_via {
                ends.push(t.points[0]);
            }
            for p in ends {
                painter.circle_filled(p, 3.4, BG);
                painter.circle_stroke(p, 3.4, Stroke::new(1.1, via_color));
            }
        }

        for c in &self.chips {
            if !c.rect.intersects(screen) {
                continue;
            }
            for [a, b] in &c.pins {
                painter.line_segment([*a, *b], Stroke::new(2.0, alpha(GOLD, 60)));
            }
            painter.rect_filled(c.rect, 1.5, Color32::from_rgb(5, 13, 8));
            painter.rect_stroke(c.rect, 1.5, Stroke::new(1.0, alpha(GREEN, 55)));
            painter.circle_filled(c.rect.left_top() + Vec2::new(6.0, 6.0), 1.6, alpha(GOLD, 90));
            painter.text(c.rect.center(), Align2::CENTER_CENTER, &c.label, FontId::monospace(9.0), alpha(GREEN, 70));
        }

        if animate {
            for t in &self.traces {
                let Some((speed, phase)) = t.pulse else { continue };
                if !t.bbox.intersects(screen) {
                    continue;
                }
                let period = t.total + 260.0;
                let d = (time * speed + phase) % period;
                if d > t.total {
                    continue;
                }
                let base = if t.gold { GOLD } else { GREEN };
                for k in 0..7 {
                    let back = d - k as f32 * 5.0;
                    if back < 0.0 {
                        break;
                    }
                    let p = Self::point_at(t, back);
                    let a = (140.0 * (1.0 - k as f32 / 7.0)) as u8;
                    painter.circle_filled(p, 1.6, alpha(base, a));
                }
                let p = Self::point_at(t, d);
                painter.circle_filled(p, 9.0, alpha(base, 14));
                painter.circle_filled(p, 5.0, alpha(base, 40));
                painter.circle_filled(p, 2.2, alpha(base, 230));
            }
        }

        // Vignette: darken the edges so the content in the middle stays readable.
        let edge = Color32::from_rgba_unmultiplied(0, 0, 0, 170);
        let clear = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
        let h = screen.height() * 0.22;
        gradient(painter, Rect::from_min_size(screen.min, Vec2::new(screen.width(), h)), edge, clear, true);
        gradient(
            painter,
            Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - h), screen.max),
            clear,
            edge,
            true,
        );
        let w = screen.width() * 0.12;
        gradient(painter, Rect::from_min_size(screen.min, Vec2::new(w, screen.height())), edge, clear, false);
        gradient(
            painter,
            Rect::from_min_max(Pos2::new(screen.right() - w, screen.top()), screen.max),
            clear,
            edge,
            false,
        );
    }
}

/// Two-color gradient rectangle. `vertical`: first color at the top, else at the left.
pub fn gradient(painter: &Painter, rect: Rect, first: Color32, second: Color32, vertical: bool) {
    let mut mesh = Mesh::default();
    let (tl, tr, bl, br) = if vertical { (first, first, second, second) } else { (first, second, first, second) };
    mesh.colored_vertex(rect.left_top(), tl);
    mesh.colored_vertex(rect.right_top(), tr);
    mesh.colored_vertex(rect.left_bottom(), bl);
    mesh.colored_vertex(rect.right_bottom(), br);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(Shape::mesh(mesh));
}
