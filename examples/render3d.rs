use del_msh_core::vtx2xyz::transform;

fn main() {
    let (tri2vtx, vtx2xyz, vtx2uv) = {
        let mut obj = del_msh_core::io_obj::WavefrontObj::<usize, f32>::new();
        obj.load("examples/asset/spot_triangulated.obj");
        let (tri2uni, uni2vtx_xyz, uni2vtx_uv) =
            del_msh_core::unify_index::unify_two_indices_of_triangle_mesh(
                &obj.idx2vtx_xyz,
                &obj.idx2vtx_uv,
            );
        assert_eq!(uni2vtx_xyz.len(), uni2vtx_uv.len());
        let uni2xyz =
            del_msh_core::map_idx::map_vertex_attibute_from(&obj.vtx2xyz, 3, &uni2vtx_xyz);
        let uni2uv = del_msh_core::map_idx::map_vertex_attibute_from(&obj.vtx2uv, 2, &uni2vtx_uv);
        (tri2uni, uni2xyz, uni2uv)
    };
    let img_size = (400, 400);
    let cam_projection = del_geo_core::mat4_col_major::camera_perspective_blender(
        img_size.0 as f32 / img_size.1 as f32,
        24f32,
        0.5,
        3.0,
    );
    let cam_modelview =
        del_geo_core::mat4_col_major::camera_external_blender(&[0., 0., 2.], 10., -30., 110.);
    let transform_world2ndc =
        del_geo_core::mat4_col_major::multmat(&cam_projection, &cam_modelview);
    let transform_ndc2world =
        del_geo_core::mat4_col_major::try_inverse(&transform_world2ndc).unwrap();
    /*
    {
        let vtx2xyz2 = del_msh_core::vtx2xyz::transform(&vtx2xyz, &transform_world2ndc);
        del_msh_core::io_obj::save_tri2vtx_vtx2xyz("target/hoge.obj", &tri2vtx, &vtx2xyz2, 3);
    }
     */
    let bvhnodes = del_msh_core::bvhnodes_morton::from_triangle_mesh(&tri2vtx, &vtx2xyz, 3);
    let aabbs = del_msh_core::aabbs3::from_uniform_mesh_with_bvh(
        0,
        &bvhnodes,
        Some((&tri2vtx, 3)),
        &vtx2xyz,
        None,
    );
    let pix2tri = del_canvas::raycast_trimesh3::pix2tri(
        &tri2vtx,
        &vtx2xyz,
        &bvhnodes,
        &aabbs,
        &img_size,
        &transform_ndc2world,
    );

    {
        let pix2rgb = del_canvas::raycast_trimesh3::render_normalmap_pix2tri(
            img_size,
            &cam_modelview,
            &tri2vtx,
            &vtx2xyz,
            &pix2tri,
        );
        del_canvas::write_png_from_float_image_rgb(
            "target/render3d_normalmap.png",
            &img_size,
            &pix2rgb,
        );
    }

    {
        let mut img_data = vec![0f32; img_size.0 * img_size.1];
        del_canvas::raycast_trimesh3::render_depth_bvh(
            img_size,
            &mut img_data,
            &transform_ndc2world,
            &tri2vtx,
            &vtx2xyz,
            &bvhnodes,
            &aabbs,
        );
        del_canvas::write_png_from_float_image_grayscale(
            "target/render3d_depth.png",
            &img_size,
            &img_data,
        );
    }
}
