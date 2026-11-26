#include "circular_reveal_item.h"

#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QSGClipNode>
#include <QSGGeometry>
#include <QSGSimpleTextureNode>

#include <algorithm>
#include <cmath>

CircularRevealItem::CircularRevealItem(QQuickItem* parent)
	: QQuickItem(parent)
{
	setFlag(ItemHasContents, true);
}

void CircularRevealItem::setCenter(const QPointF& center)
{
	if(m_center == center)
	{
		return;
	}
	m_center = center;
	emit centerChanged();
	update();
}

void CircularRevealItem::setRadius(qreal radius)
{
	if(qFuzzyCompare(m_radius + 1.0, radius + 1.0))
	{
		return;
	}
	m_radius = radius;
	emit radiusChanged();
	update();
}

void CircularRevealItem::setInverted(bool inverted)
{
	if(m_inverted == inverted)
	{
		return;
	}
	m_inverted = inverted;
	emit invertedChanged();
	update();
}

void CircularRevealItem::setGrabResult(QObject* result)
{
	const auto* grab = qobject_cast<QQuickItemGrabResult*>(result);
	m_image = grab ? grab->image() : QImage();
	m_textureDirty = true;
	update();
}

void CircularRevealItem::clear()
{
	if(m_image.isNull())
	{
		return;
	}
	m_image = QImage();
	m_textureDirty = true;
	update();
}

QSGNode* CircularRevealItem::updatePaintNode(QSGNode* oldNode, UpdatePaintNodeData* /*data*/)
{
	if(m_image.isNull() || width() <= 0.0 || height() <= 0.0 || !window())
	{
		delete oldNode;
		return nullptr;
	}

	const double w = static_cast<double>(width());
	const double h = static_cast<double>(height());
	const double cx = static_cast<double>(m_center.x());
	const double cy = static_cast<double>(m_center.y());
	const double dxMax = std::max(std::abs(cx), std::abs(w - cx));
	const double dyMax = std::max(std::abs(cy), std::abs(h - cy));
	const double maxCornerDist = std::hypot(dxMax, dyMax);
	const double r = std::max(0.0, static_cast<double>(m_radius));

	// Avoid allocating a 0-vertex QRhiBuffer when the clipped region is completely empty.
	if((!m_inverted && r <= 0.5) || (m_inverted && r >= maxCornerDist))
	{
		delete oldNode;
		return nullptr;
	}

	auto* clipNode = static_cast<QSGClipNode*>(oldNode);
	if(!clipNode)
	{
		clipNode = new QSGClipNode();
		clipNode->setIsRectangular(false);
		auto* clipGeom = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 0);
		clipGeom->setDrawingMode(QSGGeometry::DrawTriangles);
		clipNode->setGeometry(clipGeom);
		clipNode->setFlag(QSGNode::OwnsGeometry, true);

		auto* texNode = new QSGSimpleTextureNode();
		texNode->setOwnsTexture(true);
		texNode->setFiltering(QSGTexture::Linear);
		clipNode->appendChildNode(texNode);
		m_textureDirty = true;
	}

	auto* texNode = static_cast<QSGSimpleTextureNode*>(clipNode->firstChild());
	if(m_textureDirty || !texNode->texture())
	{
		QSGTexture* texture = window()->createTextureFromImage(m_image);
		if(!texture)
		{
			delete clipNode;
			return nullptr;
		}
		texNode->setTexture(texture);
		m_textureDirty = false;
	}
	const QRectF bounds = boundingRect();
	texNode->setRect(bounds);
	clipNode->setClipRect(bounds);

	auto* clipGeom = clipNode->geometry();
	constexpr int Segments = 96;
	constexpr double TwoPi = 6.28318530717958647692;

	if(!m_inverted)
	{
		// Keep inside circle (shrinking old light snapshot to reveal new dark underneath).
		clipGeom->allocate(Segments * 3);
		auto* verts = clipGeom->vertexDataAsPoint2D();
		const float fcx = static_cast<float>(cx);
		const float fcy = static_cast<float>(cy);
		for(int i = 0; i < Segments; ++i)
		{
			const double a0 = TwoPi * static_cast<double>(i) / static_cast<double>(Segments);
			const double a1 = TwoPi * static_cast<double>(i + 1) / static_cast<double>(Segments);
			const float x0 = static_cast<float>(cx + r * std::cos(a0));
			const float y0 = static_cast<float>(cy + r * std::sin(a0));
			const float x1 = static_cast<float>(cx + r * std::cos(a1));
			const float y1 = static_cast<float>(cy + r * std::sin(a1));
			verts[i * 3 + 0].set(fcx, fcy);
			verts[i * 3 + 1].set(x0, y0);
			verts[i * 3 + 2].set(x1, y1);
		}
	}
	else
	{
		// Keep outside circle (expanding hole in old dark snapshot to reveal new light underneath).
		const double inner = r;
		const double outer = std::max(maxCornerDist + 4.0, inner + 4.0);
		clipGeom->allocate(Segments * 6);
		auto* verts = clipGeom->vertexDataAsPoint2D();
		for(int i = 0; i < Segments; ++i)
		{
			const double a0 = TwoPi * static_cast<double>(i) / static_cast<double>(Segments);
			const double a1 = TwoPi * static_cast<double>(i + 1) / static_cast<double>(Segments);
			const double c0 = std::cos(a0);
			const double s0 = std::sin(a0);
			const double c1 = std::cos(a1);
			const double s1 = std::sin(a1);
			const float ix0 = static_cast<float>(cx + inner * c0);
			const float iy0 = static_cast<float>(cy + inner * s0);
			const float ox0 = static_cast<float>(cx + outer * c0);
			const float oy0 = static_cast<float>(cy + outer * s0);
			const float ix1 = static_cast<float>(cx + inner * c1);
			const float iy1 = static_cast<float>(cy + inner * s1);
			const float ox1 = static_cast<float>(cx + outer * c1);
			const float oy1 = static_cast<float>(cy + outer * s1);
			const int base = i * 6;
			verts[base + 0].set(ix0, iy0);
			verts[base + 1].set(ox0, oy0);
			verts[base + 2].set(ox1, oy1);
			verts[base + 3].set(ix0, iy0);
			verts[base + 4].set(ox1, oy1);
			verts[base + 5].set(ix1, iy1);
		}
	}

	clipNode->markDirty(QSGNode::DirtyGeometry);
	return clipNode;
}
