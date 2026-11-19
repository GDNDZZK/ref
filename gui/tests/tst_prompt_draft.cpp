#include "compose/prompt_draft.h"

#include <QTest>

using namespace Qt::StringLiterals;
using refgui::proto::NamedImage;
using refgui::compose::PromptDraft;

namespace
{

NamedImage image(const QString& data, std::optional<QString> name = std::nullopt)
{
	return {.mime = u"image/png"_s, .data = data, .name = std::move(name)};
}

QStringList dataOf(const PromptDraft& draft)
{
	QStringList data;
	for(const auto& attachment : draft.attachments())
	{
		data.append(attachment.token + u"="_s + attachment.image.data);
	}
	return data;
}

} // namespace

class TestPromptDraft : public QObject
{
	Q_OBJECT

private slots:
	void attachInsertsTokenAtCursor();
	void attachPadsAfterText();
	void tokensResetWhenDraftHasNoAttachments();
	void editingTheTextDropsOrphanedImages();
	void removingAnAttachmentRemovesItsToken();
	void takeReturnsTextAndImages();
	void restoreFillsAnEmptyDraft();
	void restoreAppendsAfterABlankLineAndRebinds();
	void restoreGivesLeftoverImagesATokenAtTheEnd();
	void restoreIgnoresNothing();
	void atomicPlaceholderSnappingSelectionAndDeletion();
};

void TestPromptDraft::attachInsertsTokenAtCursor()
{
	PromptDraft draft;
	draft.attach(image(u"a"_s));
	QCOMPARE(draft.text(), u"[Image 1] "_s);
	QCOMPARE(draft.cursor(), 10);
	QCOMPARE(dataOf(draft), QStringList{u"[Image 1]=a"_s});
}

void TestPromptDraft::attachPadsAfterText()
{
	PromptDraft draft;
	draft.setText(u"look at"_s, 7);
	draft.attach(image(u"a"_s));
	QCOMPARE(draft.text(), u"look at [Image 1] "_s);

	draft.setText(u"line\n"_s, 5);
	draft.attach(image(u"b"_s));
	QCOMPARE(draft.text(), u"line\n[Image 1] "_s);
}

void TestPromptDraft::tokensResetWhenDraftHasNoAttachments()
{
	PromptDraft draft;
	draft.attach(image(u"a"_s));
	QCOMPARE(draft.text(), u"[Image 1] "_s);

	// clear() resets numbering.
	draft.clear();
	draft.attach(image(u"b"_s));
	QCOMPARE(draft.text(), u"[Image 1] "_s);

	// take() empties the draft via clear() and resets numbering.
	QCOMPARE(draft.take().images.size(), 1);
	draft.attach(image(u"c"_s));
	QCOMPARE(draft.text(), u"[Image 1] "_s);

	// Partial removeAttachment() keeps rising; full removal resets.
	draft.attach(image(u"d"_s));
	QCOMPARE(dataOf(draft), (QStringList{u"[Image 1]=c"_s, u"[Image 2]=d"_s}));
	draft.removeAttachment(0);
	draft.attach(image(u"e"_s));
	QCOMPARE(dataOf(draft), (QStringList{u"[Image 2]=d"_s, u"[Image 3]=e"_s}));
	draft.removeAttachment(0);
	draft.removeAttachment(0);
	QVERIFY(draft.attachments().isEmpty());
	draft.attach(image(u"f"_s));
	QCOMPARE(draft.text(), u"[Image 1] "_s);

	// Partial setText()/dropOrphans() keeps rising; dropping all tokens resets.
	draft.attach(image(u"g"_s));
	draft.setText(u"[Image 2] "_s, 10);
	draft.attach(image(u"h"_s));
	QCOMPARE(dataOf(draft), (QStringList{u"[Image 2]=g"_s, u"[Image 3]=h"_s}));
	draft.setText(u"plain text"_s, 10);
	QVERIFY(draft.attachments().isEmpty());
	draft.attach(image(u"i"_s));
	QCOMPARE(draft.text(), u"plain text [Image 1] "_s);

	// deletePlaceholderAt() drops the last attachment and resets numbering.
	QVERIFY(draft.deletePlaceholderAt(20, true));
	QCOMPARE(draft.text(), u"plain text "_s);
	QVERIFY(draft.attachments().isEmpty());
	draft.attach(image(u"j"_s));
	QCOMPARE(draft.text(), u"plain text [Image 1] "_s);

	// restore() renumbers into the current box sequence without colliding, and resets after clear().
	draft.clear();
	draft.restore(u"see [Image 5]"_s, {image(u"r"_s)});
	QCOMPARE(draft.text(), u"see [Image 1]"_s);
	draft.attach(image(u"k"_s));
	QCOMPARE(dataOf(draft), (QStringList{u"[Image 1]=r"_s, u"[Image 2]=k"_s}));
	draft.clear();
	draft.restore(u"just text"_s, {});
	draft.attach(image(u"l"_s));
	QCOMPARE(draft.text(), u"just text [Image 1] "_s);
}

void TestPromptDraft::editingTheTextDropsOrphanedImages()
{
	PromptDraft draft;
	draft.attach(image(u"a"_s));
	draft.attach(image(u"b"_s));
	QCOMPARE(draft.attachments().size(), 2);

	draft.setText(u"only [Image 2] left"_s, 19);
	QCOMPARE(dataOf(draft), QStringList{u"[Image 2]=b"_s});
}

void TestPromptDraft::removingAnAttachmentRemovesItsToken()
{
	PromptDraft draft;
	draft.setText(u"see "_s, 4);
	draft.attach(image(u"a"_s));
	draft.setText(draft.text() + u"and more"_s, draft.text().size() + 8);
	QCOMPARE(draft.text(), u"see [Image 1] and more"_s);

	draft.removeAttachment(0);
	QCOMPARE(draft.text(), u"see and more"_s);
	QVERIFY(draft.attachments().isEmpty());
	QCOMPARE(draft.cursor(), draft.text().size());
}

void TestPromptDraft::takeReturnsTextAndImages()
{
	PromptDraft draft;
	draft.attach(image(u"a"_s));
	draft.setText(draft.text() + u"what is this"_s, draft.text().size() + 12);

	const auto submission = draft.take();
	QCOMPARE(submission.text, u"[Image 1] what is this"_s);
	QCOMPARE(submission.images.size(), 1);
	QCOMPARE(submission.images[0].data, u"a"_s);
	QVERIFY(draft.isEmpty());
	QCOMPARE(draft.cursor(), 0);
}

void TestPromptDraft::restoreFillsAnEmptyDraft()
{
	PromptDraft draft;
	draft.restore(u"fix it"_s, {});
	QCOMPARE(draft.text(), u"fix it"_s);
	QCOMPARE(draft.cursor(), 6);
}

// The same case as the TUI's editor test.
void TestPromptDraft::restoreAppendsAfterABlankLineAndRebinds()
{
	PromptDraft draft;
	draft.setText(u"draft "_s, 6);
	draft.attach(image(u"a"_s));

	draft.restore(u"see [Image 2] and [Image 1]"_s, {image(u"x"_s), image(u"y"_s, u"y.png"_s)});
	QCOMPARE(draft.text(), u"draft [Image 1]\n\nsee [Image 3] and [Image 2]"_s);
	QCOMPARE(draft.cursor(), draft.text().size());
	QCOMPARE(dataOf(draft), (QStringList{u"[Image 1]=a"_s, u"[Image 2]=x"_s, u"[Image 3]=y"_s}));
	QCOMPARE(draft.attachments()[2].image.name, u"y.png"_s);
	QCOMPARE(draft.take().images.size(), 3);
}

void TestPromptDraft::restoreGivesLeftoverImagesATokenAtTheEnd()
{
	PromptDraft draft;
	draft.restore(u"look"_s, {image(u"z"_s)});
	QCOMPARE(draft.text(), u"look [Image 1] "_s);
	QCOMPARE(draft.attachments().size(), 1);
}

void TestPromptDraft::restoreIgnoresNothing()
{
	PromptDraft draft;
	draft.setText(u"keep"_s, 4);
	draft.restore(u"  "_s, {});
	QCOMPARE(draft.text(), u"keep"_s);
}
void TestPromptDraft::atomicPlaceholderSnappingSelectionAndDeletion()
{
	PromptDraft draft;
	draft.setText(u"see "_s, 4);
	draft.attach(image(u"a"_s));
	draft.setText(draft.text() + u"now"_s, draft.text().size() + 3);
	// "see [Image 1] now" -> token is [4, 13)
	QCOMPARE(draft.text(), u"see [Image 1] now"_s);
	QCOMPARE(draft.snapCursor(5, 4), 13);   // moving right from start snaps past token
	QCOMPARE(draft.snapCursor(12, 13), 4);  // moving left from end snaps before token
	QCOMPARE(draft.snapCursor(6, 6), 4);    // click near left edge snaps left
	QCOMPARE(draft.snapCursor(11, 11), 13); // click near right edge snaps right
	QCOMPARE(draft.expandSelection(2, 6), (std::pair<qsizetype, qsizetype>{2, 13}));
	QCOMPARE(draft.expandSelection(10, 15), (std::pair<qsizetype, qsizetype>{4, 15}));
	// Non-attached literal "[Image 9]" is NOT treated as an atomic attachment placeholder.
	draft.setText(u"see [Image 1] and [Image 9]"_s, 27);
	QCOMPARE(draft.snapCursor(21, 20), 21);
	QVERIFY(!draft.deletePlaceholderAt(13, false)); // after [Image 1], Delete hits " and"
	QVERIFY(draft.deletePlaceholderAt(13, true));   // Backspace at token end removes token + trailing space
	QCOMPARE(draft.text(), u"see and [Image 9]"_s);
	QVERIFY(draft.attachments().isEmpty());
}
QTEST_GUILESS_MAIN(TestPromptDraft)
#include "tst_prompt_draft.moc"
