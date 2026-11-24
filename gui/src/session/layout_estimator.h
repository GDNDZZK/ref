#pragma once

#include "proto/types.h"

#include <QVariantList>

namespace refgui::session
{

// Pure local layout estimation from lightweight MessageLayoutMeta without network I/O.
class LayoutEstimator
{
public:
	static constexpr double DefaultColumnWidth = 760.0;
	static constexpr double MinColumnWidth = 180.0;
	static constexpr double MinMessageHeight = 40.0;

	static proto::MessageLayoutMeta computeMeta(quint64 seq, const proto::Message& message);

	static double estimateMessageHeight(const proto::MessageLayoutMeta& meta,
	                                    double columnWidth,
	                                    bool thinkingExpanded,
	                                    bool detailsExpanded);

	static QVariantList skeletonParts(const proto::MessageLayoutMeta& meta);
};

} // namespace refgui::session
