# Adora Nano model snapshot

This directory contains the local CAD revision available to Nexus Studio. It is
self-contained: `adora_nano.urdf`, `adora_nano_floating.xml`, and `meshes/` use
relative paths. No symlink or sibling checkout is required.

The model is based on [DoraCN/AdoraNano](https://github.com/DoraCN/AdoraNano)
commit `43f94e2ae32352464e056f25a4aedb7912136669`, with the supplied October 31
arm CAD update. `provenance.json` records input hashes and the path adaptation;
mesh bytes and joint definitions are unchanged from that CAD revision.

The chassis and head retain the upstream meshes. Their comparison with the
October 22 CAD was sampled, not an exact part-by-part match. Masses and inertias
remain nominal. The floating XML is a collision inspection model without
ground, actuators or visual meshes; it has small zero-pose contact overlaps.
The synthetic Studio clips are for viewing joint motion, not hardware control.

Source STEP files and local CAD analysis reports are not included.

In Nexus Studio, choose **Robots → Import** and select `adora_nano.urdf`.
Import copies referenced assets into the local library and checks the URDF.
A matching simulation task and actuator mapping are required before training.
