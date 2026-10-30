#include "composer/shared_history.h"

#include "client/instance_files.h"

using namespace Qt::StringLiterals;

namespace refgui::composer
{

compose::PromptHistory& sharedPromptHistory()
{
	static compose::PromptHistory history(client::stateDirectory() + u"/prompt-history.jsonl"_s);
	return history;
}

} // namespace refgui::composer
