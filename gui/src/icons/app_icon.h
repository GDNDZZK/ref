#pragma once

#include <QIcon>
#include <QImage>
#include <QList>

namespace refgui::icons
{

// The pixel sizes of the program icon, the same as in assets/app/ref.ico.
inline const QList<int> AppIconSizes = {16, 24, 32, 48, 64, 128, 256};
// Up to this size the icon is the small "r." mark (ref-small.svg), above it the four-pane mark
// (ref.svg), as in ref.ico.
constexpr int AppIconSmallMarkMaxSize = 48;

// The program icon at one of AppIconSizes, or a null image for a size it does not have.
QImage appIconImage(int pixels);

// The program icon with an image at each of AppIconSizes. The SVGs are drawn with QSvgRenderer,
// so no image-format or icon-engine plugin has to be in the package.
QIcon appIcon();

} // namespace refgui::icons
