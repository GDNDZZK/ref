// Prints Qt's easing curves sampled at progress i / SAMPLES as JSON, for src/motion/easing.test.ts.
// The formulas are Qt's own: easing.cpp is included unmodified (see generate.sh). What this file
// adds is the part of QEasingCurve (src/corelib/tools/qeasingcurve.cpp) that wraps them:
// progress is clamped to [0, 1] (valueForProgress), BackEase pins both ends, and ElasticEase
// falls back to Qt's defaults for a negative period or amplitude.

#include "easing.cpp"

#include <algorithm>
#include <cstdio>
#include <functional>
#include <string>
#include <vector>

namespace
{

constexpr int SAMPLES = 1000;
constexpr double DefaultOvershoot = 1.70158;

struct Curve
{
	std::string name;
	double amplitude;
	double period;
	std::function<double(double)> value;
};

double clampProgress(double progress)
{
	return std::min(1.0, std::max(0.0, progress));
}

double backEase(double t, double overshoot)
{
	if(!(t > 0))
	{
		return 0;
	}
	if(!(t < 1))
	{
		return 1;
	}
	return easeOutBack(t, overshoot);
}

Curve elastic(double amplitude, double period)
{
	return {"outElastic", amplitude, period, [=](double t) { return easeOutElastic(t, amplitude, period); }};
}

} // namespace

int main()
{
	const std::vector<Curve> curves = {
		{"linear", 0, 0, [](double t) { return easeNone(t); }},
		{"inQuad", 0, 0, [](double t) { return easeInQuad(t); }},
		{"inCubic", 0, 0, [](double t) { return easeInCubic(t); }},
		{"inOutSine", 0, 0, [](double t) { return easeInOutSine(t); }},
		{"outExpo", 0, 0, [](double t) { return easeOutExpo(t); }},
		{"outBack", 0, 0, [](double t) { return backEase(t, DefaultOvershoot); }},
		elastic(1.0, 0.3),   // Qt defaults
		elastic(2.0, 0.35),  // Flourish.springAmplitude / springPeriod
		elastic(1.4, 0.35),  // Flourish.entrySpringAmplitude
		elastic(0.5, 0.3),   // amplitude below 1: raised to 1
	};

	std::printf("[\n");
	for(std::size_t c = 0; c < curves.size(); ++c)
	{
		const Curve& curve = curves[c];
		std::printf("  {\"name\": \"%s\", \"amplitude\": %g, \"period\": %g, \"samples\": [", curve.name.c_str(), curve.amplitude,
					curve.period);
		for(int i = 0; i <= SAMPLES; ++i)
		{
			std::printf("%s%.17g", i == 0 ? "" : ", ", curve.value(clampProgress(static_cast<double>(i) / SAMPLES)));
		}
		std::printf("]}%s\n", c + 1 < curves.size() ? "," : "");
	}
	std::printf("]\n");
}
