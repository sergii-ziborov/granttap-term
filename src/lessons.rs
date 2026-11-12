#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillKind {
    Vim,
    Split,
    Kubectl,
    Files,
    Say,
}

pub struct Drill {
    pub title: &'static str,
    pub brief: &'static str,
    pub kind: DrillKind,
}

pub struct Lesson {
    pub section: &'static str,
    pub title: &'static str,
    pub parts: &'static [(&'static str, &'static [&'static str])],
    pub drills: &'static [Drill],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Head(&'static str),
    Lesson(usize),
    Part(&'static str),
    Line(&'static str),
    Drill(usize, usize),
    Brief(&'static str),
    Gap,
}

impl Row {
    pub fn picks(self) -> bool {
        matches!(self, Self::Lesson(_) | Self::Drill(_, _))
    }

    pub fn text(self) -> String {
        let lessons = lessons();
        match self {
            Self::Head(name) | Self::Part(name) => format!("\x1b[38;5;220m{name}"),
            Self::Lesson(index) => format!("  {}", lessons[index].title),
            Self::Line(text) | Self::Brief(text) => format!("\x1b[38;5;131m  {text}"),
            Self::Drill(lesson, drill) => {
                format!("\x1b[38;5;51mpractice  {}", lessons[lesson].drills[drill].title)
            }
            Self::Gap => String::new(),
        }
    }
}

pub fn lessons() -> &'static [Lesson] {
    &[
        Lesson {
            section: "Desk",
            title: "The desk",
            parts: &[
                (
                    "Stay",
                    &[
                        "An open chat stays open on every tab.",
                        "Files opens that chat's mesh.",
                    ],
                ),
                (
                    "Place",
                    &[
                        "The desk runs in this terminal.",
                        "An SSH session is the same desk.",
                    ],
                ),
            ],
            drills: &[
                Drill {
                    title: "Open Files",
                    brief: "Files shows the open chat's mesh.",
                    kind: DrillKind::Files,
                },
                Drill {
                    title: "Send a line",
                    brief: "A typed line goes to the open chat.",
                    kind: DrillKind::Say,
                },
            ],
        },
        Lesson {
            section: "Agents",
            title: "Claude",
            parts: &[
                (
                    "List",
                    &[
                        "Claude, Codex, and Grok share one list.",
                        "Each agent keeps its own ten chats.",
                    ],
                ),
                (
                    "Commands",
                    &[
                        "An open chat suggests that agent's commands.",
                        "Enter sends the line to the live session.",
                    ],
                ),
            ],
            drills: &[Drill {
                title: "Send a line",
                brief: "Open a chat, type a line, press Enter.",
                kind: DrillKind::Say,
            }],
        },
        Lesson {
            section: "Vim",
            title: "Vim",
            parts: &[
                ("Open", &["On Files, Enter opens the file in vim."]),
                (
                    "Edit",
                    &[
                        "Type to change a line.",
                        "Enter breaks the line. Backspace deletes.",
                    ],
                ),
                (
                    "Write",
                    &[
                        "F2 or F4 writes the file.",
                        "Esc leaves vim. The open chat comes back.",
                    ],
                ),
            ],
            drills: &[Drill {
                title: "Scratch file",
                brief: "Change the second line. F2 writes. Esc returns.",
                kind: DrillKind::Vim,
            }],
        },
        Lesson {
            section: "Terminal",
            title: "The terminal",
            parts: &[
                ("Run", &["Enter runs a command in this tab."]),
                (
                    "Split",
                    &["split opens another shell.", "shell switches between shells."],
                ),
                (
                    "Tools",
                    &["After kubectl or git, its commands join the suggestions."],
                ),
            ],
            drills: &[
                Drill {
                    title: "Split a shell",
                    brief: "Opens a second shell in this tab.",
                    kind: DrillKind::Split,
                },
                Drill {
                    title: "kubectl",
                    brief: "Suggestions gain get, describe, and logs.",
                    kind: DrillKind::Kubectl,
                },
            ],
        },
        Lesson {
            section: "Kubernetes",
            title: "Kubernetes",
            parts: &[
                (
                    "Where",
                    &[
                        "Kubernetes lives in the terminal.",
                        "There is no pod screen and no Docker screen.",
                    ],
                ),
                (
                    "Commands",
                    &[
                        "kubectl get pods lists the pods.",
                        "kubectl logs reads a container.",
                    ],
                ),
            ],
            drills: &[Drill {
                title: "kubectl",
                brief: "Opens the terminal with kubectl suggestions.",
                kind: DrillKind::Kubectl,
            }],
        },
    ]
}

pub fn rows(open: Option<usize>) -> Vec<Row> {
    let lessons = lessons();
    let Some(index) = open else {
        let mut out = Vec::new();
        let mut last = "";
        for (index, lesson) in lessons.iter().enumerate() {
            if lesson.section != last {
                out.push(Row::Head(lesson.section));
                last = lesson.section;
            }
            out.push(Row::Lesson(index));
        }
        return out;
    };
    let Some(lesson) = lessons.get(index) else {
        return Vec::new();
    };
    let mut out = vec![Row::Head(lesson.title)];
    for (title, lines) in lesson.parts {
        out.push(Row::Part(title));
        for line in *lines {
            out.push(Row::Line(line));
        }
    }
    out.push(Row::Gap);
    for (drill, item) in lesson.drills.iter().enumerate() {
        out.push(Row::Drill(index, drill));
        out.push(Row::Brief(item.brief));
    }
    out
}
