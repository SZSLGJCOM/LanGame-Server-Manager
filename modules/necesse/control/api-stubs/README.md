These are minimal compile-time API declarations, independently written from the
public signatures in the fixed Necesse 1.3.3 package. They contain no game logic.
The final artifact contains only `langame/necesse` classes and owned resources;
no `necesse` declaration classes, ByteBuddy or JNA code is packaged.

Runtime reflection and the native package profile remain authoritative. The
native SDK compilation and isolated native-class fixture verify that these
references resolve against the real game classes. The explicit throwing bodies
make accidental use of a declaration fail loudly.
