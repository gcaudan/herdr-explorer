//! The branch picker of the source control pane, and the names typed in place.

use super::{BOLD, dim, spaces, split, sw, tw};
use crate::core::ui::{self, Rgb, Seg, bg, c, fg, seg};
use crate::scm::Pane;
use crate::scm::context::{self};
use crate::scm::state;

impl Pane {
    // --- rows of the views
    /// A menu in a frame, as wide as its branches (72 columns at most): the switch's hangs under the header's branch,
    /// merge's and rebase's under the Actions menu that opened them (p.anchor, its right edge), shifted to stay on
    /// screen. The lines keep what they showed on both sides of it.
    pub(super) fn draw_picker(&mut self, lines: &mut Vec<String>, w: i64, h: i64) {
        let edge = dim();
        let head_name = self.repo().map_or(String::new(), |r| state::head_name(r).to_string());
        let pill_x = self.frame.pill_x;
        let inline_branch = self.inline.as_ref().is_some_and(|it| it.kind == "branch");
        let p = self.s.picker.as_mut().expect("a picker to draw");
        let title = match p.mode {
            Some("merge") => format!("Merge into {head_name}"),
            Some("rebase") => format!("Rebase {head_name} onto"),
            _ => "Switch branch".into(),
        };
        let width = |b: &context::PickItem| match b {
            context::PickItem::Create(name) => 3 + tw(name),
            context::PickItem::Branch(b) => {
                let b = &b.branch;
                3 + tw(&format!("{}  {}", b.name, b.subject)) + tw(&format!(" {} ", b.date)) + 1
            }
        };
        let want = p.items.iter().map(width).fold((tw(&title) + 4).max(30), i64::max);
        let inner = (w - 2).min(70).min(want);
        let x = match p.anchor {
            Some((right, _)) => (right - inner - 2).max(0),
            None => pill_x.min(w - inner - 2).max(0),
        };
        let y = p.anchor.map_or(1, |a| a.1);
        let bh = 4.max((h - 1 - y).min(p.items.len() as i64 + 3));
        let body = bh - 3;
        if p.sel < p.top {
            p.top = p.sel;
        }
        if p.sel >= p.top + body {
            p.top = p.sel - body + 1;
        }
        p.r#box = (x, y, inner + 2, bh, body);
        let (top, sel) = (p.top, p.sel);
        let items = p.items.clone();
        let search: Vec<Seg> = if p.typing {
            p.field.segments(inner - 4, "")
        } else {
            vec![seg(
                if p.field.value.is_empty() { "type / to filter".to_string() } else { p.field.value.clone() },
                dim(),
            )]
        };
        while (lines.len() as i64) < h - 1 {
            lines.push(ui::line(&[], w as usize, None));
        }
        let iw = inner.max(0) as usize;
        let mut boxed = Vec::new();
        let head = format!(" {} ", ui::ellipsis(&title, inner - 4));
        boxed.push(ui::line(
            &[
                seg("╭─", edge.clone()),
                seg(head.clone(), BOLD),
                seg(format!("{}╮", "─".repeat((inner - 1 - tw(&head)).max(0) as usize)), edge.clone()),
            ],
            (inner + 2).max(0) as usize,
            None,
        ));
        let side = |l: String| format!("{edge}│\x1b[0m{l}{edge}│\x1b[0m");
        let mut first = vec![seg(format!(" {} ", ui::g().search), dim())];
        first.extend(search);
        boxed.push(side(ui::line(&first, iw, Some(c().panel))));
        for i in top..top + body {
            let Some(b) = items.get(i.max(0) as usize).filter(|_| i >= 0) else {
                boxed.push(side(ui::line(&[], iw, Some(c().menu))));
                continue;
            };
            let back = Some(if i == sel { c().select } else { c().menu });
            boxed.push(side(match b {
                context::PickItem::Create(_) if inline_branch => self.inline_row(inner, " + ", Some(c().menu)),
                context::PickItem::Create(name) => {
                    ui::line(&[seg(" + ", fg(c().green)), seg(name, fg(c().green))], iw, back)
                }
                context::PickItem::Branch(pick) => {
                    let b = &pick.branch;
                    let style = if b.remote {
                        fg(c().purple)
                    } else if pick.current {
                        BOLD.into()
                    } else {
                        String::new()
                    };
                    split(
                        &[
                            seg(if pick.current { " ● " } else { "   " }, fg(c().green)),
                            seg(&b.name, style),
                            seg(format!("  {}", b.subject), dim()),
                        ],
                        &[seg(format!(" {} ", b.date), dim())],
                        inner,
                        back,
                    )
                }
            }));
        }
        let count = items.iter().filter(|b| matches!(b, context::PickItem::Branch(_))).count();
        let foot = format!(" {count} branch{} ", if count == 1 { "" } else { "es" });
        boxed.push(ui::line(
            &[
                seg("╰─", edge.clone()),
                seg(foot.clone(), dim()),
                seg(format!("{}╯", "─".repeat((inner - 1 - tw(&foot)).max(0) as usize)), edge.clone()),
            ],
            (inner + 2).max(0) as usize,
            None,
        ));
        for (i, l) in boxed.into_iter().enumerate() {
            let at = (y.max(0) as usize) + i;
            while lines.len() <= at {
                lines.push(ui::line(&[], w as usize, None));
            }
            let old = lines[at].clone();
            let before = if x > 0 { ui::head(&old, x as usize) } else { String::new() };
            lines[at] = format!("{before}{l}{}", ui::tail(&old, (x + inner + 2).max(0) as usize));
        }
    }

    /// The row of the name being typed, width w, on back.
    pub(super) fn inline_row(&self, w: i64, lead: &str, back: Option<Rgb>) -> String {
        let Some(it) = &self.inline else { return String::new() };
        let right = format!(" {} ", it.right);
        let field_bg = bg(c().field);
        let fw = (w - tw(lead) - tw(&right) - 1).max(4);
        let field: Vec<Seg> =
            it.field.segments(fw - 1, "").into_iter().map(|(t, st)| seg(t, format!("{field_bg}{st}"))).collect();
        let mut segs = vec![seg(lead, dim()), seg(" ", field_bg.clone())];
        let used = sw(&field);
        segs.extend(field);
        segs.push(seg(spaces(fw - 1 - used), field_bg));
        segs.push(seg(spaces((w - tw(lead) - fw - tw(&right)).max(1)), ""));
        segs.push(seg(right, dim()));
        ui::line(&segs, w.max(0) as usize, back)
    }
}
