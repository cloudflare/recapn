# recapnc

recapnc is the compiler plugin for generating compatible recapn code. It generates files using quote
and syn, with prettification providied by the 'prettyplease' crate. It's probably the best example
of what using recapn looks like in practice.

Some things I wanted to improve over capnp-rust with this plugin is:

1. Don't use strings for generating code.
2. Automatically provide a module layout for the generated code that can be placed anywhere in the
   module tree.

## Overview

The plugin is divided into 2 distinct sections:

* Quotes, which basically describes all generated code output and what's required to generate
  specific code. It effectively acts as an outline of the generated output.
* Generator, which gathers and validates code generator request data. This ultimately creates the
  quotes which are used to make the generated code output.

This two stage form makes it extremely easy to break up and think about changes to generate code.
The average change looks something like this:

1. Create your generate code output by adding onto a generated item's `to_tokens!` implementation.
2. Backfill all required variables needed for the generated code output into the generated item's
   struct.
3. Gather and retain required information from the code generator request and provide it to the
   generated item.