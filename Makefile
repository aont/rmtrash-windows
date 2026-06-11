CXX ?= g++
PREFIX ?= /ucrt64
TARGET ?= rmtrash.exe
SRC := src/rmtrash.cpp

CPPFLAGS ?=
CXXFLAGS ?= -std=c++17 -O2 -Wall -Wextra
LDFLAGS ?=
LDLIBS := -lshell32

.PHONY: all clean install

all: $(TARGET)

$(TARGET): $(SRC)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -o $@ $< $(LDFLAGS) $(LDLIBS)

install: $(TARGET)
	install -d "$(DESTDIR)$(PREFIX)/bin"
	install -m 755 "$(TARGET)" "$(DESTDIR)$(PREFIX)/bin/rmtrash.exe"

clean:
	rm -f "$(TARGET)"
