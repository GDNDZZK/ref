//! Scripts that only talk.

use crate::mock::script::{Ctx, Pace, Round, Script};

const MARKDOWN: &str = include_str!("../assets/markdown.md");
const ARTICLE: &str = include_str!("../assets/article.md");
/// The slow script shows this much of the article, about a minute at `Pace::SLOW`.
const SLOW_CHARS: usize = 1700;

pub const SCRIPTS: &[Script] = &[
    Script {
        name: "help",
        about: "Lists all scripts",
        pace: Pace::NORMAL,
        play: help,
    },
    Script {
        name: "hello",
        about: "A one-line reply",
        pace: Pace::NORMAL,
        play: hello,
    },
    Script {
        name: "default",
        about: "What any other text gets: an echo of the prompt",
        pace: Pace::NORMAL,
        play: default,
    },
    Script {
        name: "markdown",
        about: "Headings, lists, tables, quotes, links and code blocks in several languages",
        pace: Pace::NORMAL,
        play: markdown,
    },
    Script {
        name: "reasoning",
        about: "A long reasoning block, then the answer",
        pace: Pace::NORMAL,
        play: reasoning,
    },
    Script {
        name: "article",
        about: "A long article with many code blocks (for scrolling checks)",
        pace: Pace::NORMAL,
        play: article,
    },
    Script {
        name: "slow",
        about: "Very slow streaming, about a minute (for frame-by-frame checks)",
        pace: Pace::SLOW,
        play: slow,
    },
];

fn help(_: &Ctx) -> Round {
    let mut table = String::from(
        "Type one of these words as your prompt:\n\n| Prompt | What it shows |\n|:--|:--|\n",
    );
    for script in super::all() {
        table.push_str(&format!("| `{}` | {} |\n", script.name, script.about));
    }
    table.push_str("\nAnything else gets the `default` reply.");
    Round::new().text(table)
}

fn hello(_: &Ctx) -> Round {
    Round::new().text("Hello! This is the mock backend. Type `help` to see what it can do.")
}

fn default(ctx: &Ctx) -> Round {
    let quoted: String = ctx.prompt.lines().map(|l| format!("> {l}\n")).collect();
    Round::new().text(format!(
        "You wrote:\n\n{quoted}\nThat is not the name of a script, so this is the default reply. Type `help` for the list of scripts.\n\n- Streaming: this text arrives in small pieces.\n- Markdown: **bold**, *italic*, `code`.\n"
    ))
}

fn markdown(_: &Ctx) -> Round {
    Round::new().text(MARKDOWN)
}

fn reasoning(_: &Ctx) -> Round {
    let thinking = "The user wants to see a reasoning block, so first I should work out what is worth showing. \
A reasoning block is usually longer than the answer and is shown dimmed and collapsible.\n\n\
Step one: list the things that can go wrong while streaming, such as layout jumps, flicker when a block closes and scroll anchors that drift.\n\n\
Step two: pick an order that keeps the reader calm, which means the newest text goes at the bottom and everything above stays where it is.\n\n\
Step three: write a short answer that does not repeat all of this.";
    Round::new()
        .reasoning(thinking)
        .text("Reasoning is shown above and collapsed once the answer starts. The answer itself is short on purpose.")
}

fn article(_: &Ctx) -> Round {
    Round::new().text(ARTICLE)
}

fn slow(_: &Ctx) -> Round {
    let end = ARTICLE
        .char_indices()
        .nth(SLOW_CHARS)
        .map_or(ARTICLE.len(), |(i, _)| i);
    Round::new().text(&ARTICLE[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::script::Step;

    #[test]
    fn help_lists_every_script() {
        let Step::Text(text) = &help(&super::super::tests::ctx("help", 0)).steps[0] else {
            panic!("help is text");
        };
        for script in super::super::all() {
            assert!(
                text.contains(&format!("`{}`", script.name)),
                "{}",
                script.name
            );
        }
    }
}
