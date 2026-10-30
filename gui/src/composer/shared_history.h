#pragma once

#include "compose/prompt_history.h"

namespace refgui::composer
{

// The prompt history of this program, which every input box shares and which lives in the
// state directory next to the TUI's, so both remember the same prompts. Read on first use.
compose::PromptHistory& sharedPromptHistory();

} // namespace refgui::composer
