// Just enough of Qt for src/3rdparty/easing/easing.cpp to compile without Qt.
#pragma once

#include <cmath>

using qreal = double;

#define QT_USE_NAMESPACE

inline double qPow(double x, double y) { return std::pow(x, y); }
inline double qSin(double v) { return std::sin(v); }
inline double qCos(double v) { return std::cos(v); }
inline double qAsin(double v) { return std::asin(v); }
inline double qFabs(double v) { return std::fabs(v); }
template <typename T> constexpr const T& qMin(const T& a, const T& b) { return (a < b) ? a : b; }
template <typename T> constexpr const T& qMax(const T& a, const T& b) { return (a < b) ? b : a; }
