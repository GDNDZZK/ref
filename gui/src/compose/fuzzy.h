#pragma once

#include <QString>

#include <optional>

namespace refgui::compose
{

// How well `needle` matches `haystack` as a case-insensitive subsequence: higher is better,
// nullopt when it does not match. Consecutive letters and letters after a separator score
// more, a prefix match most. The same scoring as the TUI's command list.
std::optional<int> fuzzyScore(const QString& haystack, const QString& needle);

} // namespace refgui::compose
