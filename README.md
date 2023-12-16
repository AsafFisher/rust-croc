<p align="center">
<img
    src="https://github.com/AsafFisher/rust-croc/assets/6796683/bbd34164-e144-4dfe-87b4-c2c97c58a612"
    width="408px" border="0" alt="croc">
    

<br>
</p>

`rust-croc` is the rust-made equivilant to `croc`, A tool that allows any two computers to simply and securely transfer files and folders. AFAIK, *croc* is the only CLI file-transfer tool that does **all** of the following:

This is a very early WIP, to start just type:
```
RUST_BACKTRACE=1 RUST_LOG=trace cargo run --release
```

- allows **any two computers** to transfer data (using a relay)
- provides **end-to-end encryption** (using PAKE)
- enables easy **cross-platform** transfers (Windows, Linux, Mac)
- allows **multiple file** transfers
- allows **resuming transfers** that are interrupted
- local server or port-forwarding **not needed**
- **ipv6-first** with ipv4 fallback

For more information about `croc`, see [Zack schollz's blog post](https://schollz.com/software/croc6).
