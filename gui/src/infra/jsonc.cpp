#include "infra/jsonc.h"

#include <optional>
#include <utility>

using namespace Qt::StringLiterals;

// JSONC is lowered to plain JSON by overwriting comments and trailing commas with spaces.
// Byte offsets never move, so offsets reported by QJsonDocument map 1:1 back onto the source:
//
//   {"a": 1, // note      ->   {"a": 1
//   }                     ->   }

namespace qutie::jsonc
{
namespace
{

constexpr char Blank = ' ';
constexpr qsizetype NoPendingComma = -1;

// Scanner states. Only `Code` sees JSON tokens; the others skip over string or comment bytes.
//
//   Code --"--> String --\--> StringEscape --(any byte)--> String --"--> Code
//   Code --//-> LineComment  --\n--> Code
//   Code --/*-> BlockComment --*/--> Code
enum class State
{
	Code,
	String,
	StringEscape,
	LineComment,
	BlockComment,
};

bool isJsonWhitespace(char c)
{
	return c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

// A ',' right after these tokens is never a trailing comma. Leaving it in place lets
// QJsonDocument reject `[,]` and `[1,,]` as it should.
bool precedesElement(char token)
{
	return token == '[' || token == '{' || token == ',';
}

bool closesContainer(char token)
{
	return token == ']' || token == '}';
}

// Rewrites `text` from JSONC to JSON in place, in a single pass.
// Comment bytes become spaces except '\n', so line numbers survive.
// Returns the start offset of an unterminated block comment, if any.
std::optional<qsizetype> lowerToJson(QByteArray& text)
{
	State state = State::Code;
	qsizetype commentStart = 0;
	char lastToken = '\0';                   // last JSON token byte seen in `Code`
	qsizetype pendingComma = NoPendingComma; // blanked if the next token closes a container

	for(qsizetype i = 0; i < text.size(); ++i)
	{
		const char c = text[i];
		const char next = i + 1 < text.size() ? text[i + 1] : '\0';

		switch(state)
		{
		case State::Code:
		{
			if(isJsonWhitespace(c))
			{
				break;
			}

			const bool opensComment = c == '/' && (next == '/' || next == '*');
			if(opensComment)
			{
				state = next == '/' ? State::LineComment : State::BlockComment;
				commentStart = i;
				text[i] = Blank;
				text[i + 1] = Blank;
				++i;
				break;
			}

			if(pendingComma != NoPendingComma && closesContainer(c))
			{
				text[pendingComma] = Blank;
			}
			const bool mayTrail = c == ',' && !precedesElement(lastToken);
			pendingComma = mayTrail ? i : NoPendingComma;
			lastToken = c;

			if(c == '"')
			{
				state = State::String;
			}
			break;
		}

		case State::String:
			if(c == '\\')
			{
				state = State::StringEscape;
			}
			else if(c == '"')
			{
				state = State::Code;
			}
			break;

		case State::StringEscape:
			state = State::String; // the escaped byte can never end the string
			break;

		case State::LineComment:
			if(c == '\n')
			{
				state = State::Code;
			}
			else
			{
				text[i] = Blank;
			}
			break;

		case State::BlockComment:
			if(c == '*' && next == '/')
			{
				text[i] = Blank;
				text[i + 1] = Blank;
				++i;
				state = State::Code;
			}
			else if(c != '\n')
			{
				text[i] = Blank;
			}
			break;
		}
	}

	if(state == State::BlockComment)
	{
		return commentStart;
	}
	return std::nullopt;
}

ParseError makeError(QByteArrayView source, qsizetype offset, QString message)
{
	const QByteArrayView before = source.first(offset);
	const qsizetype lineStart = before.lastIndexOf('\n') + 1;

	const int line = static_cast<int>(before.count('\n')) + 1;
	const int column = static_cast<int>(QString::fromUtf8(before.sliced(lineStart)).size()) + 1;
	return {line, column, std::move(message)};
}

} // namespace

std::expected<QJsonDocument, ParseError> parse(QByteArrayView source)
{
	QByteArray text = source.toByteArray();

	if(const std::optional<qsizetype> unterminated = lowerToJson(text))
	{
		return std::unexpected(makeError(source, *unterminated, u"unterminated block comment"_s));
	}

	QJsonParseError jsonError;
	QJsonDocument document = QJsonDocument::fromJson(text, &jsonError);
	if(jsonError.error != QJsonParseError::NoError)
	{
		return std::unexpected(makeError(source, jsonError.offset, jsonError.errorString()));
	}
	return document;
}

} // namespace qutie::jsonc
