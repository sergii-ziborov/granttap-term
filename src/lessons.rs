use crate::model::Lesson;

pub fn lessons() -> &'static [Lesson] {
    &[
        Lesson {
            title: "The desk",
            body: "GrantTap Term runs inside the terminal you already have, including an SSH session. The client machine does not need this program or a graphical mode.",
        },
        Lesson {
            title: "Claude",
            body: "Chats from Claude, Codex, and Grok on this computer are one list. move writes a handoff and opens that chat in the other agent's split.",
        },
        Lesson {
            title: "The terminal",
            body: "Suggestions sit above the input. Every command has a description line under it. Control+V pastes an image as [Image #N], and the cell preview can be opened.",
        },
        Lesson {
            title: "Kubernetes",
            body: "Kubernetes lives in lessons. This desk has no pod screen and no Docker screen.",
        },
    ]
}
