# SPDX-License-Identifier: GPL-3.0-or-later
# Shared compile settings for every qutie-owned target (GCC / Clang).

option(QUTIE_SANITIZE "Build with AddressSanitizer and UndefinedBehaviorSanitizer" OFF)

function(qutie_configure_target target)
	target_compile_options(${target} PRIVATE
		-Wall
		-Wextra
		-Wpedantic
		-Wshadow
		-Wnon-virtual-dtor
		-Woverloaded-virtual
		-Wold-style-cast
		-Wcast-qual
		-Wconversion
		-Wimplicit-fallthrough
	)
	# Override locally with `cmake --compile-no-warning-as-error`.
	set_target_properties(${target} PROPERTIES COMPILE_WARNING_AS_ERROR ON)

	target_compile_definitions(${target} PRIVATE
		# Disables Qt APIs deemed deprecated or error-prone up to 6.10 (see qtconfigmacros.h).
		QT_ENABLE_STRICT_MODE_UP_TO=0x060A00
		# Every char* -> QString conversion must state its encoding.
		QT_NO_CAST_FROM_ASCII
	)

	if(QUTIE_SANITIZE)
		target_compile_options(${target} PRIVATE -fsanitize=address,undefined -fno-omit-frame-pointer)
		target_link_options(${target} PRIVATE -fsanitize=address,undefined)
	endif()
endfunction()
