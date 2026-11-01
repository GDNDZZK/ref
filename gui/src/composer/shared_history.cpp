#include "composer/shared_history.h"

#include "workspace/state_paths.h"

using namespace Qt::StringLiterals;

namespace refgui::composer
{

compose::PromptHistory& sharedPromptHistory()
{
	static compose::PromptHistory history(workspace::stateFilePath(u"prompt-history.jsonl"_s));
	return history;
}

} // namespace refgui::composer
