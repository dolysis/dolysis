# Overall Design Philosophy

This is a set of generic design ideals.

## Table of Contents

- [Overall Design Philosophy](#overall-design-philosophy)
  - [Table of Contents](#table-of-contents)
  - [The Big Three](#the-big-three)
    - [Simple should be Simple](#simple-should-be-simple)
      - [Example](#example)
        - [C++](#c)
        - [Perl](#perl)
    - [Complex should be Possible](#complex-should-be-possible)
    - [Transition must be Easy](#transition-must-be-easy)

## The Big Three

To make a comparison I will use `C++` and `Perl`, this is purely to make an example and I chose these precisely to demonstrate the point and show that just because something follows these rules, that does not mean it is good or better.

### Simple should be Simple

To start and do something simple it should be easy and not require a complex configuration.

#### Example

Print `Hello World!` on the terminal.

##### C++

```cpp
// the cpp source file hello-world.cpp
#include <iostream>

using namespace std;

int main()
{
    cout << "Hello World!" << endl;
    return 0;
}
```

```bash
# Compile and Run C++

clang -o hello-world hello-world.cpp
./hello-world

```

##### Perl

```bash
# For Perl

perl -E 'say "Hello World!"'

```

### Complex should be Possible

### Transition must be Easy
