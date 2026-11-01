# Bundled models

These ONNX models are embedded into the binary and run locally (no network) with
[tract](https://github.com/sonos/tract). They come from the
[OpenCV Model Zoo](https://github.com/opencv/opencv_zoo).

| File | Purpose | License |
|---|---|---|
| `face_detection_yunet_2023mar.onnx` | Face detection + 5 landmarks (eyes, nose, mouth corners) — YuNet by Shiqi Yu et al. | MIT, see `LICENSE-yunet-MIT.txt` |
| `image_classification_mobilenetv2_2022apr.onnx` | ImageNet-1k classifier, used to recognise the scene (landscape, city, food, animal, still life) | Apache-2.0, see `LICENSE-mobilenet-Apache-2.0.txt` |

SHA-256
- `face_detection_yunet_2023mar.onnx` 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4
- `image_classification_mobilenetv2_2022apr.onnx` c0c3f76d93fa3fd6580652a45618618a220fced18babf65774ed169de0432ad5
