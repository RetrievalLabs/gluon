# System Flow

- Client source files are cloned into an isolated VM.
- Client source lives on a separate attached volume so project state is decoupled from the VM lifecycle.
- The VM includes all required build tools, Gluon tools, language servers, and migration support utilities.
