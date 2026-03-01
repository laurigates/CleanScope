# Changelog

## [0.2.2](https://github.com/laurigates/CleanScope/compare/clean-scope-v0.2.1...clean-scope-v0.2.2) (2026-03-01)


### Bug Fixes

* **ci:** pass explicit value to --aab flag in Tauri Android build ([#42](https://github.com/laurigates/CleanScope/issues/42)) ([83c0fb5](https://github.com/laurigates/CleanScope/commit/83c0fb5222f83ae1d7e0852a31e7f0754392aced))

## [0.2.1](https://github.com/laurigates/CleanScope/compare/clean-scope-v0.2.0...clean-scope-v0.2.1) (2026-03-01)


### Bug Fixes

* **ci:** chain Android release into release-please workflow ([#40](https://github.com/laurigates/CleanScope/issues/40)) ([252ce57](https://github.com/laurigates/CleanScope/commit/252ce5703b2fec36fdc70013869340f0fef1e8e0))

## [0.2.0](https://github.com/laurigates/CleanScope/compare/clean-scope-v0.1.0...clean-scope-v0.2.0) (2026-03-01)


### Features

* add ADB WiFi shortcuts and improve UI status messages ([aafedbd](https://github.com/laurigates/CleanScope/commit/aafedbd14f4e3119bdabeb0129f8f7d96d0e04d8))
* add Android emulator and project init recipes ([78c1a66](https://github.com/laurigates/CleanScope/commit/78c1a6685df2eae07b5ba4612094282c5a726fb0))
* add build info display and isochronous USB streaming ([6955220](https://github.com/laurigates/CleanScope/commit/695522030da2b2837a6d4710cbb06875fcb2a13f))
* add CleanScope-specific Claude commands and skills ([9250903](https://github.com/laurigates/CleanScope/commit/92509032905231406b5bf1b6bbc0f8dae95efe29))
* add development workflow recipes to justfile ([078958e](https://github.com/laurigates/CleanScope/commit/078958e42258f862facddf5139342b7e3ecf1ec1))
* add E2E testing infrastructure with USB packet capture and replay ([#1](https://github.com/laurigates/CleanScope/issues/1)) ([0fbb2b6](https://github.com/laurigates/CleanScope/commit/0fbb2b6c1093996cf8e5cad2e1f33cdca936edbb))
* **ci:** add Google Play Store GitOps pipeline ([#37](https://github.com/laurigates/CleanScope/issues/37)) ([024cc2c](https://github.com/laurigates/CleanScope/commit/024cc2ce7c711452e85283278de9553d94a1b283))
* implement format index cycling to find MJPEG format ([75a37a4](https://github.com/laurigates/CleanScope/commit/75a37a4dc66bc7909e7ae31c65461493ca963d78))
* implement frame streaming with polling pattern ([0c315ca](https://github.com/laurigates/CleanScope/commit/0c315ca24c3b036ff023a9cd4a91fded354c422a))
* implement shared frame state for isochronous USB transfers ([6d9a46e](https://github.com/laurigates/CleanScope/commit/6d9a46e1a1b97287d7cab4628e2d1c2897d4bd9d))
* implement UVC camera streaming foundation with libusb ([96be497](https://github.com/laurigates/CleanScope/commit/96be4973aa05910278b5f35621778c484a07205e))
* improve USB device permission handling and intent processing ([c59358b](https://github.com/laurigates/CleanScope/commit/c59358bbf4a66066b63bf6b96f20d0e245cefa2d))
* **resolution:** implement resolution cycling for UVC cameras ([1d2694f](https://github.com/laurigates/CleanScope/commit/1d2694f0726b4d9a384c9c4dbba06101345ce85c))
* scaffold Tauri v2 + Rust Android app for USB endoscope viewing ([ef8a8fe](https://github.com/laurigates/CleanScope/commit/ef8a8fe329db9a270b02e71c897f80bc6b02d2d9))
* **skills:** add USB camera analysis skill and commands ([a1d3ee1](https://github.com/laurigates/CleanScope/commit/a1d3ee1f3d450237b3a2713f7e5027b22ec7371f))
* **ui:** add video format controls to debug toolbar ([55cec8f](https://github.com/laurigates/CleanScope/commit/55cec8f4b6338cc69bff1663d44919237af96acb))
* **usb:** implement USB disconnection error handling (F016) ([6b4b173](https://github.com/laurigates/CleanScope/commit/6b4b17308aa6df92f9fc9b2a1ca6c9acdd29b8d7))
* **usb:** improve isochronous transfers, reconnection events, and tooling ([#36](https://github.com/laurigates/CleanScope/issues/36)) ([f2bd0a3](https://github.com/laurigates/CleanScope/commit/f2bd0a36a8bd82eae7ee9bbd8a785cc16391a9c0))
* use UVC descriptors for resolution and support RGB frame rendering ([3ca56d4](https://github.com/laurigates/CleanScope/commit/3ca56d40a20d5f03ab7215cf53ad610437e80b38))
* **video:** add frame validation and extended pixel format support ([d1d5235](https://github.com/laurigates/CleanScope/commit/d1d523514616836dc3e63097cebaefd561739f5b))


### Bug Fixes

* resolve code anti-patterns from static analysis ([#35](https://github.com/laurigates/CleanScope/issues/35)) ([8976039](https://github.com/laurigates/CleanScope/commit/8976039be587241c8e6759991a7237483a928fea))
* resolve video freeze by disabling unreliable FID frame detection ([9bdc70f](https://github.com/laurigates/CleanScope/commit/9bdc70fde982785bdd0460efd3349f8167cf42c4))
* **security:** remove withGlobalTauri to reduce XSS attack surface ([#39](https://github.com/laurigates/CleanScope/issues/39)) ([85b1c08](https://github.com/laurigates/CleanScope/commit/85b1c080e96df46edfbfca3dab67c8d7f881a60f))
* trust UVC descriptors for frame size detection ([5322ba2](https://github.com/laurigates/CleanScope/commit/5322ba210e2bb394a2347ee66fb0e4f599d48c1d))


### Performance Improvements

* include frame info in frame-ready event payload ([#26](https://github.com/laurigates/CleanScope/issues/26)) ([0f40655](https://github.com/laurigates/CleanScope/commit/0f406551d156ce0c99fb73049203f65651ead5c1))
