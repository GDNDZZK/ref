#pragma once

#include <QImage>
#include <QPointF>
#include <QQuickItem>
#include <QtQmlIntegration>

// Renders a grabbed QImage clipped to an expanding circular hole (inverted = true)
// or a shrinking circular disc (inverted = false) around `center`.
class CircularRevealItem : public QQuickItem
{
	Q_OBJECT
	QML_ELEMENT
	Q_PROPERTY(QPointF center READ center WRITE setCenter NOTIFY centerChanged FINAL)
	Q_PROPERTY(qreal radius READ radius WRITE setRadius NOTIFY radiusChanged FINAL)
	Q_PROPERTY(bool inverted READ isInverted WRITE setInverted NOTIFY invertedChanged FINAL)

public:
	explicit CircularRevealItem(QQuickItem* parent = nullptr);

	QPointF center() const
	{
		return m_center;
	}
	void setCenter(const QPointF& center);

	qreal radius() const
	{
		return m_radius;
	}
	void setRadius(qreal radius);

	bool isInverted() const
	{
		return m_inverted;
	}
	void setInverted(bool inverted);
	Q_INVOKABLE void setGrabResult(QObject* result);
	Q_INVOKABLE void clear();

signals:
	void centerChanged();
	void radiusChanged();
	void invertedChanged();

protected:
	QSGNode* updatePaintNode(QSGNode* oldNode, UpdatePaintNodeData* data) override;

private:
	QPointF m_center;
	qreal m_radius = 0.0;
	bool m_inverted = false;
	QImage m_image;
	bool m_textureDirty = false;
};
