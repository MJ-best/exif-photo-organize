# Bundled models

These ONNX models are embedded into the binary and run locally (no network) with
[tract](https://github.com/sonos/tract). They come from the
[OpenCV Model Zoo](https://github.com/opencv/opencv_zoo).

| File | Purpose | License |
|---|---|---|
| `face_detection_yunet_2023mar.onnx` | Face detection + 5 landmarks (eyes, nose, mouth corners) — YuNet by Shiqi Yu et al. | MIT, see `LICENSE-yunet-MIT.txt` |
| `image_classification_mobilenetv2_2022apr.onnx` | ImageNet-1k classifier, used to recognise the scene (landscape, city, food, animal, still life) | Apache-2.0, see `LICENSE-mobilenet-Apache-2.0.txt` |
| `human_segmentation_pphumanseg_2023mar.onnx` | Person segmentation (PP-HumanSeg from PaddleSeg, 192×192), separates people from the background for the composition check | Apache-2.0, see `LICENSE-pphumanseg-Apache-2.0.txt` |

SHA-256
- `face_detection_yunet_2023mar.onnx` 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4
- `image_classification_mobilenetv2_2022apr.onnx` c0c3f76d93fa3fd6580652a45618618a220fced18babf65774ed169de0432ad5
- `human_segmentation_pphumanseg_2023mar.onnx` 552d8a984054e59b5d773d24b9b12022b22046ceb2bbc4c9aaeaceb36a9ddf24
